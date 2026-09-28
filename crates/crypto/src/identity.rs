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
    fn random_identities_differ() {
        assert_ne!(Identity::generate().peer_id(), Identity::generate().peer_id());
    }
}
