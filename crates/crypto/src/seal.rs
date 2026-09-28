//! Sealed boxes between two known static keys (§14.4): signalling that a room owner forwards
//! between members M and Y without being able to read or alter it.
//!
//! `key = HKDF-BLAKE2s(X25519(static_M, static_Y), "ephem/seal/1" ‖ min(pk) ‖ max(pk) ‖ context)`,
//! box = `nonce 12 ‖ ChaCha20-Poly1305(plain) ‖ tag 16` with a random nonce. Static-static, so no
//! forward secrecy: acceptable for signalling, whose content (ICE parameters) is short-lived and
//! whose authenticity is pinned again by the Noise KK handshake that follows.

use crate::identity::{Identity, PeerId};
use blake2::Blake2s256;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use hkdf::SimpleHkdf;
use zeroize::Zeroize;

pub const OVERHEAD: usize = 12 + 16;

fn key(me: &Identity, peer: &PeerId, context: &[u8]) -> [u8; 32] {
    let mut shared = me.dh(peer);
    let (lo, hi) = if me.peer_id() < *peer { (me.peer_id(), *peer) } else { (*peer, me.peer_id()) };
    let hk = SimpleHkdf::<Blake2s256>::new(None, &shared);
    let mut out = [0u8; 32];
    hk.expand_multi_info(&[b"ephem/seal/1", &lo.0, &hi.0, context], &mut out).expect("32-byte HKDF output");
    shared.zeroize();
    out
}

/// Seals `plain` for `peer`, bound to `context` (e.g. room id ‖ sender idx ‖ receiver idx).
pub fn seal(me: &Identity, peer: &PeerId, context: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut k = key(me, peer, context);
    let mut nonce = [0u8; 12];
    crate::random(&mut nonce);
    let mut out = Vec::with_capacity(OVERHEAD + plain.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(plain);
    let tag = ChaCha20Poly1305::new(Key::from_slice(&k))
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), context, &mut out[12..])
        .expect("in-memory encryption cannot fail");
    out.extend_from_slice(&tag);
    k.zeroize();
    out
}

/// Opens a box sealed by `peer` for us with the same `context`; `None` if altered or not for us.
pub fn open(me: &Identity, peer: &PeerId, context: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < OVERHEAD {
        return None;
    }
    let mut k = key(me, peer, context);
    let (nonce, rest) = sealed.split_at(12);
    let (ct, tag) = rest.split_at(rest.len() - 16);
    let mut plain = ct.to_vec();
    let ok = ChaCha20Poly1305::new(Key::from_slice(&k))
        .decrypt_in_place_detached(Nonce::from_slice(nonce), context, &mut plain, Tag::from_slice(tag))
        .is_ok();
    k.zeroize();
    ok.then_some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open() {
        let (m, y, o) = (Identity::from_seed(&[1; 32]), Identity::from_seed(&[2; 32]), Identity::from_seed(&[3; 32]));
        let b = seal(&m, &y.peer_id(), b"ctx", b"ice params");
        assert_eq!(open(&y, &m.peer_id(), b"ctx", &b).as_deref(), Some(&b"ice params"[..]));
        assert!(open(&o, &m.peer_id(), b"ctx", &b).is_none(), "the forwarding owner cannot open it");
        assert!(open(&y, &m.peer_id(), b"ctX", &b).is_none(), "bound to its context");
        let mut t = b.clone();
        t[15] ^= 1;
        assert!(open(&y, &m.peer_id(), b"ctx", &t).is_none(), "altered");
    }
}
