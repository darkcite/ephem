// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! What a poster signs (G.5.1 `s`) and the signing domains (G.4): a signature for one kind
//! never verifies as another.

use crate::{BoardError, limits};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ephem_channel::cbor::{self, Value};

/// A poster's post or thread.
pub const POST: &[u8] = b"ephem-board-post-v1:";
/// A post shown under the owner's capcode (BC-3: an owner post signed as a plain post never
/// verifies as a capcode post, and the reverse).
pub const CAP: &[u8] = b"ephem-board-cap-v1:";
/// An owner (or, later, janitor) action.
pub const ACT: &[u8] = b"ephem-board-act-v1:";
/// A poster deleting their own post.
pub const SELF_DELETE: &[u8] = b"ephem-board-del-v1:";
/// A report.
pub const REPORT: &[u8] = b"ephem-board-report-v1:";
/// The board's manifest.
pub const MANIFEST: &[u8] = b"ephem-board-manifest-v1:";

/// The poster-signed part of a post (`s`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    /// The board's IPNS name (`k51…`): a post cannot be moved to another board.
    pub b: String,
    /// The thread it replies to; 0 for a new thread.
    pub t: u64,
    /// The poster key (per post, or per thread with IDs on).
    pub k: [u8; 32],
    /// A fresh random nonce per post: binds the proof of work to this post (G.8).
    pub n: [u8; 16],
    /// Subject (new threads only).
    pub sub: String,
    pub body: String,
    /// Do not bump the thread.
    pub sage: bool,
    /// The proof-of-work epoch it was made in.
    pub e: u32,
    /// `k` is the poster's trip key (stable on this board, G.4), not a per-post key: readers
    /// show it as `!` + 16 characters ([`trip_text`]).
    pub trip: bool,
}

impl Signed {
    pub fn to_value(&self) -> Value {
        cbor::map(vec![
            ("b", Value::Text(self.b.clone())),
            ("t", Value::Uint(self.t)),
            ("k", Value::Bytes(self.k.to_vec())),
            ("n", Value::Bytes(self.n.to_vec())),
            ("sub", Value::Text(self.sub.clone())),
            ("body", Value::Text(self.body.clone())),
            ("sage", Value::Bool(self.sage)),
            ("e", Value::Uint(u64::from(self.e))),
            ("trip", Value::Bool(self.trip)),
        ])
    }

    /// Strict: every field present with its type, the limits kept, nothing else.
    pub fn from_value(v: &Value) -> Result<Self, BoardError> {
        let Value::Map(m) = v else { return Err(BoardError::Invalid) };
        if m.len() != 9 {
            return Err(BoardError::Invalid);
        }
        let text = |k: &str| v.get(k).and_then(Value::text).map(str::to_owned).ok_or(BoardError::Invalid);
        let arr = |k: &str| v.get(k).and_then(Value::bytes).ok_or(BoardError::Invalid);
        let s = Signed {
            b: text("b")?,
            t: v.get("t").and_then(Value::uint).ok_or(BoardError::Invalid)?,
            k: arr("k")?.try_into().map_err(|_| BoardError::Invalid)?,
            n: arr("n")?.try_into().map_err(|_| BoardError::Invalid)?,
            sub: text("sub")?,
            body: text("body")?,
            sage: v.get("sage").and_then(Value::boolean).ok_or(BoardError::Invalid)?,
            e: v.get("e").and_then(Value::uint).and_then(|e| u32::try_from(e).ok()).ok_or(BoardError::Invalid)?,
            trip: v.get("trip").and_then(Value::boolean).ok_or(BoardError::Invalid)?,
        };
        s.check()?;
        Ok(s)
    }

    /// Limits; a reply carries no subject.
    pub fn check(&self) -> Result<(), BoardError> {
        if self.body.len() > limits::BODY || self.sub.len() > limits::SUBJECT || (self.t != 0 && !self.sub.is_empty()) {
            return Err(BoardError::TooLong);
        }
        if self.t == 0 && self.body.is_empty() && self.sub.is_empty() {
            return Err(BoardError::Invalid);
        }
        Ok(())
    }

    /// The bytes the poster signs: `POST ‖ dag-cbor(s)` (`CAP ‖ …` for a capcode post).
    pub fn signed_bytes(&self, capcode: bool) -> Vec<u8> {
        domain(if capcode { CAP } else { POST }, &self.to_value())
    }

    /// Signs as the poster (the app does this in the poster's tab).
    pub fn sign(&self, key: &SigningKey) -> [u8; 64] {
        self.sign_as(key, false)
    }

    /// Signs as a plain post, or (`capcode`, the board key) as a capcode post.
    pub fn sign_as(&self, key: &SigningKey, capcode: bool) -> [u8; 64] {
        debug_assert_eq!(key.verifying_key().to_bytes(), self.k);
        key.sign(&self.signed_bytes(capcode)).to_bytes()
    }

    /// Verifies the poster's signature with `k`.
    pub fn verify(&self, sig: &[u8; 64]) -> Result<(), BoardError> {
        self.verify_as(sig, false)
    }

    pub fn verify_as(&self, sig: &[u8; 64], capcode: bool) -> Result<(), BoardError> {
        let key = VerifyingKey::from_bytes(&self.k).map_err(|_| BoardError::BadSignature)?;
        key.verify(&self.signed_bytes(capcode), &Signature::from_bytes(sig)).map_err(|_| BoardError::BadSignature)
    }
}

/// A trip as shown: `!` + 16 base32 characters of `BLAKE2b-80("ephem-board-trip" ‖ k)` (80 bits,
/// A-m2: 10 characters were grindable). The full key is shown on request.
pub fn trip_text(k: &[u8; 32]) -> String {
    use blake2::digest::{Update, VariableOutput};
    let mut h = blake2::Blake2bVar::new(10).expect("10-byte output");
    h.update(b"ephem-board-trip");
    h.update(k);
    let mut out = [0u8; 10];
    h.finalize_variable(&mut out).expect("10-byte output");
    let mut t = String::with_capacity(17);
    t.push('!');
    t.push_str(&ephem_channel::cid::base32(&out));
    t
}

/// `prefix ‖ dag-cbor(v)`.
pub fn domain(prefix: &[u8], v: &Value) -> Vec<u8> {
    let mut m = prefix.to_vec();
    m.extend_from_slice(&v.encode());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(t: u64, body: &str) -> (Signed, SigningKey) {
        let key = SigningKey::from_bytes(&[9; 32]);
        let s = Signed { b: "k51test".into(), t, k: key.verifying_key().to_bytes(), n: [1; 16], sub: if t == 0 { "Hi".into() } else { String::new() }, body: body.into(), sage: false, e: 7, trip: false };
        (s, key)
    }

    #[test]
    fn sign_verify_round_trip_and_domains() {
        let (s, key) = sample(0, "first");
        let sig = s.sign(&key);
        assert_eq!(s.verify(&sig), Ok(()));
        assert_eq!(Signed::from_value(&Value::decode(&s.to_value().encode()).unwrap()), Ok(s.clone()));
        let mut other = s.clone();
        other.t = 5;
        other.sub.clear();
        assert_eq!(other.verify(&sig), Err(BoardError::BadSignature), "moved to another thread");
        // The same bytes under another kind's prefix do not verify as a post.
        let act = key.sign(&domain(ACT, &s.to_value())).to_bytes();
        assert_eq!(s.verify(&act), Err(BoardError::BadSignature));
        // BC-3: a plain post's signature is no capcode post's, and the reverse.
        assert_eq!(s.verify_as(&sig, true), Err(BoardError::BadSignature));
        let cap = s.sign_as(&key, true);
        assert_eq!(s.verify_as(&cap, true), Ok(()));
        assert_eq!(s.verify(&cap), Err(BoardError::BadSignature));
    }

    #[test]
    fn limits_and_strictness() {
        let (mut s, _) = sample(3, "x");
        s.sub = "a reply has no subject".into();
        assert_eq!(s.check(), Err(BoardError::TooLong));
        let (mut s, _) = sample(0, "");
        s.body = "y".repeat(limits::BODY + 1);
        assert_eq!(s.check(), Err(BoardError::TooLong));
        let (s, _) = sample(0, "ok");
        let Value::Map(mut m) = s.to_value() else { unreachable!() };
        m.push(("zz".into(), Value::Null));
        assert_eq!(Signed::from_value(&Value::Map(m)), Err(BoardError::Invalid), "an extra field");
    }
}
