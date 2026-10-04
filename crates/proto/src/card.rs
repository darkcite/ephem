// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Contact cards (§7.5): `kind = 6`, a QR code or `#k=` link that lets someone add you as a
//! contact without chatting first, and (Tor mode) dial you once with the card's secret.
//!
//! Layout: header (version, kind 6, flags 0, 0), `peer_id [32]`, `onion_pk [32]`,
//! `card_secret [16]`, `expires_at u32` (0 = never), nickname `u8 len` + UTF-8 (≤ 32 B).
//! A card is not a connection code: it never goes through [`crate::code::Code`].

use crate::ErrorCode::{self, InvalidInvite, ProtocolMismatch};
use crate::VERSION;
use crate::buf::{Buf, Rd};

pub const KIND_CARD: u8 = 6;
pub const MAX_NICK: usize = 32;
/// Header, peer_id, onion_pk, card_secret, expires_at, nickname length.
pub const MIN_CARD_LEN: usize = 4 + 32 + 32 + 16 + 4 + 1;
pub const MAX_CARD_LEN: usize = MIN_CARD_LEN + MAX_NICK;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Card<'a> {
    pub peer_id: [u8; 32],
    pub onion_pk: [u8; 32],
    pub secret: [u8; 16],
    /// Unix seconds; 0 = never expires.
    pub expires_at: u32,
    /// Suggested nickname (UTF-8, no control characters); the receiver may change it.
    pub nick: &'a [u8],
}

/// A nickname anyone may show: ≤ 32 bytes of UTF-8 with no control characters (the adapter's
/// rows are separated by tabs and new lines: security audit F-04), no invisible or
/// direction-changing characters (they hide a suffix from the impersonation check or reorder what
/// is shown), and no check marks (the app shows ✔ after a verified name). Used for cards, HELLO
/// nicknames, contacts and one's own nickname.
pub fn valid_nick(n: &[u8]) -> bool {
    n.len() <= MAX_NICK && core::str::from_utf8(n).is_ok_and(|s| !s.chars().any(forbidden_in_nick))
}

#[inline]
fn forbidden_in_nick(c: char) -> bool {
    // As titles (crate::text), and stricter: no variation selectors, no check marks.
    c.is_control()
        || crate::text::direction(c)
        || crate::text::invisible(c)
        || matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{2713}' | '\u{2714}' | '\u{2705}' | '\u{2611}' | '\u{1F5F8}')
}

impl<'a> Card<'a> {
    /// Writes the card into `out`; returns its length.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ()> {
        if !valid_nick(self.nick) {
            return Err(());
        }
        let mut b = Buf::new(out);
        b.put(&[VERSION, KIND_CARD, 0, 0])?;
        b.put(&self.peer_id)?;
        b.put(&self.onion_pk)?;
        b.put(&self.secret)?;
        b.u32(self.expires_at)?;
        b.u8(self.nick.len() as u8)?;
        b.put(self.nick)?;
        Ok(b.len())
    }

    /// Strict decoding (borrows the nickname from `src`).
    pub fn decode(src: &'a [u8]) -> Result<Self, ErrorCode> {
        if !(MIN_CARD_LEN..=MAX_CARD_LEN).contains(&src.len()) {
            return Err(InvalidInvite);
        }
        let mut r = Rd::new(src);
        if r.u8().ok_or(InvalidInvite)? != VERSION {
            return Err(ProtocolMismatch);
        }
        if r.u8() != Some(KIND_CARD) || r.u8() != Some(0) || r.u8() != Some(0) {
            return Err(InvalidInvite);
        }
        let peer_id = r.arr::<32>().ok_or(InvalidInvite)?;
        let onion_pk = r.arr::<32>().ok_or(InvalidInvite)?;
        let secret = r.arr::<16>().ok_or(InvalidInvite)?;
        let expires_at = r.u32().ok_or(InvalidInvite)?;
        let n = r.u8().ok_or(InvalidInvite)? as usize;
        let nick = r.take(n).ok_or(InvalidInvite)?;
        if r.remaining() != 0 || !valid_nick(nick) {
            return Err(InvalidInvite);
        }
        Ok(Self { peer_id, onion_pk, secret, expires_at, nick })
    }

    /// Whether the card may still be used at `now_s` (Unix seconds).
    #[inline(always)]
    pub fn live(&self, now_s: u32) -> bool {
        self.expires_at == 0 || now_s <= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nick_rules() {
        assert!(valid_nick("Alice".as_bytes()) && valid_nick("Алиса 🙂".as_bytes()) && valid_nick(b""));
        for bad in ["a\tb", "a\nb", "Alice\u{200B}", "\u{202E}ecilA", "Alice ✔", "Alice\u{FEFF}", "x\u{E0041}"] {
            assert!(!valid_nick(bad.as_bytes()), "{bad:?}");
        }
        assert!(!valid_nick(&[0xff]) && !valid_nick(&[b'a'; 33]));
    }

    use super::*;

    fn sample(nick: &[u8]) -> Card<'_> {
        Card { peer_id: [1; 32], onion_pk: [2; 32], secret: [3; 16], expires_at: 1_800_000_000, nick }
    }

    #[test]
    fn round_trip_and_strictness() {
        let mut out = [0u8; MAX_CARD_LEN];
        let c = sample("Алиса".as_bytes());
        let n = c.encode(&mut out).unwrap();
        assert_eq!(n, MIN_CARD_LEN + "Алиса".len());
        assert_eq!(Card::decode(&out[..n]), Ok(c));
        assert_eq!(Card::decode(&out[..n - 1]), Err(InvalidInvite), "truncated");
        let mut long = [0u8; MAX_CARD_LEN + 1];
        long[..n].copy_from_slice(&out[..n]);
        assert_eq!(Card::decode(&long[..n + 1]), Err(InvalidInvite), "trailing byte");
        let mut bad = out;
        bad[0] = 9;
        assert_eq!(Card::decode(&bad[..n]), Err(ProtocolMismatch));
        let mut bad = out;
        bad[1] = 5;
        assert_eq!(Card::decode(&bad[..n]), Err(InvalidInvite), "not a card");
        let mut bad = out;
        bad[2] = 1;
        assert_eq!(Card::decode(&bad[..n]), Err(InvalidInvite), "reserved flags");
        assert!(sample(b"a\nb").encode(&mut out).is_err(), "control character");
        assert!(sample(&[0xff]).encode(&mut out).is_err(), "not UTF-8");
        assert!(sample(&[b'x'; 33]).encode(&mut out).is_err(), "too long");
        let n = sample(b"").encode(&mut out).unwrap();
        assert_eq!(n, MIN_CARD_LEN);
        assert!(Card::decode(&out[..n]).is_ok(), "no nickname");
    }

    #[test]
    fn expiry() {
        let c = sample(b"");
        assert!(c.live(1_800_000_000) && !c.live(1_800_000_001));
        assert!(Card { expires_at: 0, ..c }.live(u32::MAX), "never expires");
    }
}
