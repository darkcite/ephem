//! Short authentication string (§10.4).
//!
//! `s = BLAKE2s("p2pchat-sas" ‖ handshake_hash)`; 6 digits from `s[0..3]`, 4 emoji from `s[3..7]`.
//! The fixed 256-entry emoji table is the contiguous block U+1F400..=U+1F4FF (animals, objects);
//! the UI renders entry `b` as `U+1F400 + b` followed by U+FE0F (emoji presentation).

use blake2::{Blake2s256, Digest};

pub const EMOJI_BASE: u32 = 0x1F400;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sas {
    /// 0..=999_999, shown zero-padded to 6 digits.
    pub digits: u32,
    /// Indices into the emoji table.
    pub emoji: [u8; 4],
}

impl Sas {
    pub fn from_handshake_hash(hh: &[u8]) -> Self {
        let mut h = Blake2s256::new();
        h.update(b"p2pchat-sas");
        h.update(hh);
        let s = h.finalize();
        Self {
            digits: u32::from_le_bytes([s[0], s[1], s[2], 0]) % 1_000_000,
            emoji: [s[3], s[4], s[5], s[6]],
        }
    }

    /// `ddd ddd` as ASCII.
    pub fn digits_ascii(&self) -> [u8; 7] {
        let mut out = *b"000 000";
        let mut v = self.digits;
        for i in [6usize, 5, 4, 2, 1, 0] {
            out[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_format() {
        let s = Sas { digits: 42_007, emoji: [0; 4] };
        assert_eq!(&s.digits_ascii(), b"042 007");
        let a = Sas::from_handshake_hash(&[1; 32]);
        assert_eq!(a, Sas::from_handshake_hash(&[1; 32]));
        assert_ne!(a, Sas::from_handshake_hash(&[2; 32]));
        assert!(a.digits < 1_000_000);
    }
}
