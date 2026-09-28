//! Noise KK session (§10.1).
//!
//! snow runs only the 1-RTT handshake (setup path, may allocate). After the handshake the raw
//! split keys drive ChaCha20-Poly1305 directly, so that transport frames are:
//! - encrypted and decrypted **in place** in the caller's frame buffer (§11.6, zero copy);
//! - authenticated together with the 12-byte outer header as AAD (§11.1);
//! - numbered by an implicit 64-bit nonce that must equal the header `seq`.
//!
//! Nonce and rekey follow the Noise spec for ChaChaPoly exactly: nonce = 4 zero bytes ‖ n (LE),
//! `REKEY(k) = ENCRYPT(k, 2^64-1, ε, zeros[32])[..32]`.

use crate::identity::{Identity, PeerId};
use crate::sas::Sas;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use ephem_proto::ErrorCode;
use ephem_proto::code::MAX_CODE_LEN;
use ephem_proto::frame::{FrameType, HEADER_LEN, Header, TAG_LEN};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
pub const PROLOGUE_TAG: &[u8] = b"p2pchat/1";
/// Handshake messages carry an empty payload: 32 B ephemeral + 16 B tag.
pub const HS_MSG_LEN: usize = 48;

pub struct Handshake {
    hs: snow::HandshakeState,
}

impl Handshake {
    /// `initiator` = the offerer (§10.1). Prologue = `"p2pchat/1" ‖ invite ‖ answer`.
    pub fn new(id: &Identity, remote: &PeerId, initiator: bool, invite: &[u8], answer: &[u8]) -> Result<Self, ErrorCode> {
        debug_assert!(invite.len() <= MAX_CODE_LEN && answer.len() <= MAX_CODE_LEN);
        let mut prologue = [0u8; PROLOGUE_TAG.len() + 2 * MAX_CODE_LEN];
        let a = PROLOGUE_TAG.len();
        let b = a + invite.len();
        let c = b + answer.len();
        prologue[..a].copy_from_slice(PROLOGUE_TAG);
        prologue[a..b].copy_from_slice(invite);
        prologue[b..c].copy_from_slice(answer);
        let params = PARAMS.parse().map_err(|_| ErrorCode::CryptoFailed)?;
        let builder = snow::Builder::new(params)
            .local_private_key(id.x_secret())
            .and_then(|b| b.remote_public_key(&remote.0))
            .and_then(|b| b.prologue(&prologue[..c]))
            .map_err(|_| ErrorCode::CryptoFailed)?;
        let hs = if initiator { builder.build_initiator() } else { builder.build_responder() };
        Ok(Self { hs: hs.map_err(|_| ErrorCode::CryptoFailed)? })
    }

    #[inline]
    pub fn is_my_turn(&self) -> bool {
        self.hs.is_my_turn()
    }

    #[inline]
    pub fn is_finished(&self) -> bool {
        self.hs.is_handshake_finished()
    }

    pub fn write(&mut self, out: &mut [u8]) -> Result<usize, ErrorCode> {
        self.hs.write_message(&[], out).map_err(|_| ErrorCode::CryptoFailed)
    }

    pub fn read(&mut self, msg: &[u8]) -> Result<(), ErrorCode> {
        let mut payload = [0u8; 0];
        match self.hs.read_message(msg, &mut payload) {
            Ok(0) => Ok(()),
            _ => Err(ErrorCode::CryptoFailed),
        }
    }

    /// Consumes the finished handshake: transport keys and the SAS.
    pub fn finish(mut self) -> Result<(Transport, Sas), ErrorCode> {
        if !self.is_finished() {
            return Err(ErrorCode::CryptoFailed);
        }
        let sas = Sas::from_handshake_hash(self.hs.get_handshake_hash());
        let (i2r, r2i) = self.hs.dangerously_get_raw_split();
        let (tx, rx) = if self.hs.is_initiator() { (i2r, r2i) } else { (r2i, i2r) };
        Ok((Transport { tx_key: tx, rx_key: rx, tx_n: 0, rx_n: 0 }, sas))
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Transport {
    tx_key: [u8; 32],
    rx_key: [u8; 32],
    tx_n: u64,
    rx_n: u64,
}

#[inline(always)]
fn nonce(n: u64) -> Nonce {
    let mut b = [0u8; 12];
    b[4..].copy_from_slice(&n.to_le_bytes());
    b.into()
}

fn rekey(k: &mut [u8; 32]) {
    let mut zeros = [0u8; 32];
    // Tag is discarded by definition of REKEY.
    let _ = ChaCha20Poly1305::new(Key::from_slice(k))
        .encrypt_in_place_detached(&nonce(u64::MAX), &[], &mut zeros)
        .expect("32-byte encryption cannot fail");
    k.copy_from_slice(&zeros);
    zeros.zeroize();
}

impl Transport {
    /// Next outgoing nonce (`seq` of the next frame).
    #[inline(always)]
    pub fn tx_seq(&self) -> u64 {
        self.tx_n
    }

    /// Seals a frame in place. Layout of `frame`: `[header 12][plaintext plain_len][room for tag 16]`.
    /// Writes the header (seq = nonce), encrypts, appends the tag, returns the frame length.
    pub fn seal(&mut self, frame: &mut [u8], plain_len: usize) -> Result<usize, ErrorCode> {
        let end = HEADER_LEN + plain_len;
        if end + TAG_LEN > frame.len() || self.tx_n == u64::MAX {
            debug_assert!(self.tx_n != u64::MAX, "nonce space exhausted");
            return Err(ErrorCode::MessageTooLarge);
        }
        Header { ftype: FrameType::Transport, flags: 0, seq: self.tx_n }
            .write(frame)
            .map_err(|_| ErrorCode::MessageTooLarge)?;
        let (hdr, body) = frame.split_at_mut(HEADER_LEN);
        let tag = ChaCha20Poly1305::new(Key::from_slice(&self.tx_key))
            .encrypt_in_place_detached(&nonce(self.tx_n), hdr, &mut body[..plain_len])
            .map_err(|_| ErrorCode::CryptoFailed)?;
        body[plain_len..plain_len + TAG_LEN].copy_from_slice(&tag);
        self.tx_n += 1;
        Ok(end + TAG_LEN)
    }

    /// Opens a transport frame in place; returns the plaintext range within `frame`.
    /// Any nonce other than the expected one is fatal (ordered, reliable channel, §10.1).
    pub fn open(&mut self, frame: &mut [u8]) -> Result<core::ops::Range<usize>, ErrorCode> {
        if frame.len() < HEADER_LEN + TAG_LEN {
            return Err(ErrorCode::ProtocolMismatch);
        }
        let h = Header::read(frame)?;
        if h.ftype != FrameType::Transport {
            return Err(ErrorCode::ProtocolMismatch);
        }
        if h.seq != self.rx_n || self.rx_n == u64::MAX {
            return Err(ErrorCode::CryptoFailed);
        }
        let plain_end = frame.len() - TAG_LEN;
        let (hdr, body) = frame.split_at_mut(HEADER_LEN);
        let (ct, tag) = body.split_at_mut(plain_end - HEADER_LEN);
        ChaCha20Poly1305::new(Key::from_slice(&self.rx_key))
            .decrypt_in_place_detached(&nonce(self.rx_n), hdr, ct, Tag::from_slice(tag))
            .map_err(|_| ErrorCode::CryptoFailed)?;
        self.rx_n += 1;
        Ok(HEADER_LEN..plain_end)
    }

    /// Call right after sealing the frame that carries a REKEY record.
    #[inline]
    pub fn rekey_tx(&mut self) {
        rekey(&mut self.tx_key);
    }

    /// Call right after opening the frame that carried a REKEY record.
    #[inline]
    pub fn rekey_rx(&mut self) {
        rekey(&mut self.rx_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephem_proto::frame::MAX_FRAME;

    fn pair() -> (Transport, Sas, Transport, Sas) {
        let a = Identity::from_seed(&[1; 32]);
        let b = Identity::from_seed(&[2; 32]);
        let mut i = Handshake::new(&a, &b.peer_id(), true, b"INV", b"ANS").unwrap();
        let mut r = Handshake::new(&b, &a.peer_id(), false, b"INV", b"ANS").unwrap();
        let mut m = [0u8; 128];
        let n = i.write(&mut m).unwrap();
        assert_eq!(n, HS_MSG_LEN);
        r.read(&m[..n]).unwrap();
        let n = r.write(&mut m).unwrap();
        assert_eq!(n, HS_MSG_LEN);
        i.read(&m[..n]).unwrap();
        let (ti, si) = i.finish().unwrap();
        let (tr, sr) = r.finish().unwrap();
        (ti, si, tr, sr)
    }

    #[test]
    fn handshake_sas_and_transport() {
        let (mut ti, si, mut tr, sr) = pair();
        assert_eq!(si, sr);
        let mut f = [0u8; MAX_FRAME];
        for round in 0..3u8 {
            f[HEADER_LEN..HEADER_LEN + 5].copy_from_slice(b"hello");
            f[HEADER_LEN] = round;
            let n = ti.seal(&mut f, 5).unwrap();
            let r = tr.open(&mut f[..n]).unwrap();
            assert_eq!(&f[r.start + 1..r.end], b"ello");
            assert_eq!(f[r.start], round);
        }
        ti.rekey_tx();
        tr.rekey_rx();
        let n = ti.seal(&mut f, 1).unwrap();
        assert!(tr.open(&mut f[..n]).is_ok());
        // Reverse direction.
        let n = tr.seal(&mut f, 2).unwrap();
        assert!(ti.open(&mut f[..n]).is_ok());
    }

    #[test]
    fn tamper_replay_and_prologue() {
        let (mut ti, _, mut tr, _) = pair();
        let mut f = [0u8; 64];
        let n = ti.seal(&mut f, 4).unwrap();
        f[4] ^= 1; // header seq: AAD
        assert!(tr.open(&mut f[..n]).is_err());
        let (mut ti, _, mut tr, _) = pair();
        let n = ti.seal(&mut f, 4).unwrap();
        let copy = f;
        tr.open(&mut f[..n]).unwrap();
        let mut again = copy;
        assert_eq!(tr.open(&mut again[..n]), Err(ErrorCode::CryptoFailed), "replay");

        // Different prologue (tampered code) → handshake fails.
        let a = Identity::from_seed(&[1; 32]);
        let b = Identity::from_seed(&[2; 32]);
        let mut i = Handshake::new(&a, &b.peer_id(), true, b"INV", b"ANS").unwrap();
        let mut r = Handshake::new(&b, &a.peer_id(), false, b"INV", b"ANX").unwrap();
        let mut m = [0u8; 128];
        let n = i.write(&mut m).unwrap();
        assert!(r.read(&m[..n]).is_err());
    }
}
