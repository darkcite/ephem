//! Identity (§7.1): one 32-byte seed, keys derived with domain-separated HKDF-BLAKE2s.

use blake2::{Blake2s256, Digest};
use hkdf::SimpleHkdf;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// X25519 static public key. Not hashed.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerId(pub [u8; 32]);

impl PeerId {
    /// `anon_` + first 6 hex digits of `BLAKE2s(PeerId)`. Display only, not authentication.
    pub fn handle(&self) -> [u8; 11] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let h = Blake2s256::digest(self.0);
        let mut out = *b"anon_000000";
        for i in 0..3 {
            out[5 + 2 * i] = HEX[(h[i] >> 4) as usize];
            out[6 + 2 * i] = HEX[(h[i] & 15) as usize];
        }
        out
    }

    /// Web Lock name that keeps one identity in one tab (§7.2):
    /// `p2pchat-id-` + first 16 hex digits of `BLAKE2s(PeerId)`.
    pub fn lock_name(&self) -> [u8; 27] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let h = Blake2s256::digest(self.0);
        let mut out = *b"p2pchat-id-0000000000000000";
        for i in 0..8 {
            out[11 + 2 * i] = HEX[(h[i] >> 4) as usize];
            out[12 + 2 * i] = HEX[(h[i] & 15) as usize];
        }
        out
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Identity {
    seed: [u8; 32],
    x_secret: [u8; 32],
    #[zeroize(skip)]
    peer_id: PeerId,
    #[zeroize(skip)]
    sign_pk: [u8; 32],
}

fn derive(seed: &[u8; 32], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    SimpleHkdf::<Blake2s256>::new(None, seed)
        .expand(info, &mut out)
        .expect("32 bytes is a valid HKDF-BLAKE2s output length");
    out
}

impl Identity {
    /// A fresh temporary identity (§7.2 default).
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        crate::random(&mut seed);
        let id = Self::from_seed(&seed);
        seed.zeroize();
        id
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let mut x_secret = derive(seed, b"p2pchat/x25519");
        let xs = x25519_dalek::StaticSecret::from(x_secret);
        let peer_id = PeerId(*x25519_dalek::PublicKey::from(&xs).as_bytes());
        // Clamping is applied by x25519 at use; store the same bytes snow will receive.
        x_secret = xs.to_bytes();
        let mut ed = derive(seed, b"p2pchat/ed25519");
        let sign_pk = ed25519_dalek::SigningKey::from_bytes(&ed).verifying_key().to_bytes();
        ed.zeroize();
        Self { seed: *seed, x_secret, peer_id, sign_pk }
    }

    #[inline(always)]
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Ed25519 public key sent in HELLO (bound to the `PeerId` by the Noise channel).
    #[inline(always)]
    pub fn sign_pk(&self) -> [u8; 32] {
        self.sign_pk
    }

    #[inline(always)]
    pub(crate) fn x_secret(&self) -> &[u8; 32] {
        &self.x_secret
    }

    #[inline(always)]
    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    /// Ed25519 signature with the identity's signing key (room state, §14.2). Setup path: the
    /// signing key is re-derived from the seed and wiped after use.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        let mut ed = derive(&self.seed, b"p2pchat/ed25519");
        let sig = ed25519_dalek::SigningKey::from_bytes(&ed).sign(msg).to_bytes();
        ed.zeroize();
        sig
    }

    /// The onion service secret of Tor mode (§28.4, §28.7): an Ed25519 seed derived from the
    /// identity seed, so a saved identity keeps its `.onion` address across sessions and devices.
    /// A separate key from the signing key (no key reuse across protocols). Wipe after use.
    pub fn onion_secret(&self) -> [u8; 32] {
        derive(&self.seed, b"p2pchat/onion-ed25519")
    }

    /// Public channel number `index` (§D.3): its Ed25519 signing seed (the IPNS name is its
    /// public key) and its onion service seed. One-way from the identity seed and from each
    /// other, so a channel cannot be linked to the chat identity. The caller wipes both.
    pub fn channel_seeds(&self, index: u32) -> ([u8; 32], [u8; 32]) {
        let mut info = [0u8; 26];
        let n = b"p2pchat/channel/".len();
        info[..n].copy_from_slice(b"p2pchat/channel/");
        info[n..n + 4].copy_from_slice(&index.to_be_bytes());
        let sign = derive(&self.seed, &info[..n + 4]);
        let m = b"p2pchat/channel-onion/".len();
        info[..m].copy_from_slice(b"p2pchat/channel-onion/");
        info[m..m + 4].copy_from_slice(&index.to_be_bytes());
        (sign, derive(&self.seed, &info[..m + 4]))
    }

    /// The onion service public key (the `.onion` address is its base32 form).
    pub fn onion_pk(&self) -> [u8; 32] {
        let mut s = self.onion_secret();
        let pk = ed25519_dalek::SigningKey::from_bytes(&s).verifying_key().to_bytes();
        s.zeroize();
        pk
    }

    /// X25519 of our static key with `peer` (sealed signalling, §14.4).
    pub fn dh(&self, peer: &PeerId) -> [u8; 32] {
        let xs = x25519_dalek::StaticSecret::from(self.x_secret);
        *xs.diffie_hellman(&x25519_dalek::PublicKey::from(peer.0)).as_bytes()
    }
}

/// Verifies an Ed25519 signature made by [`Identity::sign`].
pub fn verify(sign_pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(pk) = ed25519_dalek::VerifyingKey::from_bytes(sign_pk) else { return false };
    pk.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_distinct() {
        let a = Identity::from_seed(&[1; 32]);
        let b = Identity::from_seed(&[1; 32]);
        let c = Identity::from_seed(&[2; 32]);
        assert_eq!(a.peer_id(), b.peer_id());
        assert_ne!(a.peer_id(), c.peer_id());
        assert_ne!(a.sign_pk(), a.peer_id().0);
        let h = a.peer_id().handle();
        assert!(h.starts_with(b"anon_") && h[5..].iter().all(u8::is_ascii_hexdigit));
        let l = a.peer_id().lock_name();
        assert!(l.starts_with(b"p2pchat-id-") && l[11..].iter().all(u8::is_ascii_hexdigit));
        assert_eq!(&l[11..17], &h[5..11], "same hash prefix as the handle");
    }

    #[test]
    fn sign_verify_and_dh() {
        let a = Identity::from_seed(&[1; 32]);
        let b = Identity::from_seed(&[2; 32]);
        let sig = a.sign(b"room state");
        assert!(verify(&a.sign_pk(), b"room state", &sig));
        assert!(!verify(&a.sign_pk(), b"room statE", &sig));
        assert!(!verify(&b.sign_pk(), b"room state", &sig));
        assert_eq!(a.dh(&b.peer_id()), b.dh(&a.peer_id()), "static-static DH is symmetric");
    }

    #[test]
    fn random_identities_differ() {
        assert_ne!(Identity::generate().peer_id(), Identity::generate().peer_id());
    }
}
