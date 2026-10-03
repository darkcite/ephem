// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The owner-state block (G.5.1 `own`, B-M6): what the owner's moderation needs and nobody else
//! may read, carried inside the board so a reopen (or, in BD-7, another device) keeps every ban
//! and switch. Encrypted with the owner-state key (`HKDF(seed, "p2pchat/board-own/" ‖ index)`)
//! and padded to 4 KiB steps; others see only its padded size.
//!
//! `own = nonce(24) ‖ XChaCha20-Poly1305(key, AAD = "ephem-board-own-v1" ‖ board pk,
//!        u32 len ‖ dag-cbor {v, bans, approved, known, held, sw, efforts} ‖ zero padding)`

use crate::board::cap;
use crate::limits;
use crate::pipeline::Efforts;
use crate::post::Signed;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ephem_channel::cbor::{self, Value};
use std::collections::VecDeque;

pub const NONCE: usize = 24;
const TAG: usize = 16;
const STEP: usize = 4096;
const AAD: &[u8] = b"ephem-board-own-v1";
/// Banned keys and owner-approved trips.
pub const BANS: usize = 256;
pub const APPROVED: usize = 256;
/// Trip keys with an accepted post (the trips-only switch admits these), oldest dropped.
pub const KNOWN: usize = 512;
/// Posts held for approval (pre-moderation).
pub const HELD: usize = 8;

/// A post held for the owner's approval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub s: Signed,
    pub sig: [u8; 64],
    pub cap: u8,
    /// When it arrived (Unix seconds).
    pub at: u64,
}

/// The owner's switches (G.8, G.9.1).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    pub paused: bool,
    pub threads_closed: bool,
    /// Only trip keys the board knows (or approved trips) may post.
    pub trips_only: bool,
    /// Only owner-approved trips may post.
    pub approved_only: bool,
    /// Every post is held until the owner approves it.
    pub premod: bool,
    /// The automatic panic mode switches to trips-only instead of closing new threads (R8).
    pub panic_trips: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Own {
    pub bans: Vec<[u8; 32]>,
    pub approved: Vec<[u8; 32]>,
    pub known: VecDeque<[u8; 32]>,
    pub held: Vec<Held>,
    pub sw: Switches,
    pub efforts: Efforts,
}

impl Default for Own {
    fn default() -> Self {
        Own { bans: Vec::new(), approved: Vec::new(), known: VecDeque::new(), held: Vec::new(), sw: Switches::default(), efforts: Efforts::DEFAULT }
    }
}

fn keys(v: &[[u8; 32]]) -> Value {
    Value::Array(v.iter().map(|k| Value::Bytes(k.to_vec())).collect())
}

fn read_keys(v: &Value, k: &str, max: usize) -> Option<Vec<[u8; 32]>> {
    let a = v.get(k)?.array()?;
    if a.len() > max {
        return None;
    }
    a.iter().map(|x| x.bytes().and_then(|b| b.try_into().ok())).collect()
}

impl Own {
    /// Whether trip key `k` may post under trips-only.
    pub fn trip_ok(&self, k: &[u8; 32]) -> bool {
        self.approved.contains(k) || self.known.contains(k)
    }

    /// Remembers a trip key that posted (most recent last).
    pub fn saw_trip(&mut self, k: &[u8; 32]) {
        if let Some(i) = self.known.iter().position(|x| x == k) {
            self.known.remove(i);
        } else if self.known.len() == KNOWN {
            self.known.pop_front();
        }
        self.known.push_back(*k);
    }

    pub fn encode(&self) -> Vec<u8> {
        let sw = &self.sw;
        cbor::map(vec![
            ("v", Value::Uint(1)),
            ("bans", keys(&self.bans)),
            ("approved", keys(&self.approved)),
            ("known", Value::Array(self.known.iter().map(|k| Value::Bytes(k.to_vec())).collect())),
            (
                "held",
                Value::Array(
                    self.held
                        .iter()
                        .map(|h| cbor::map(vec![("s", h.s.to_value()), ("sig", Value::Bytes(h.sig.to_vec())), ("cap", Value::Uint(u64::from(h.cap))), ("at", Value::Uint(h.at))]))
                        .collect(),
                ),
            ),
            (
                "sw",
                cbor::map(vec![
                    ("paused", Value::Bool(sw.paused)),
                    ("threads_closed", Value::Bool(sw.threads_closed)),
                    ("trips_only", Value::Bool(sw.trips_only)),
                    ("approved_only", Value::Bool(sw.approved_only)),
                    ("premod", Value::Bool(sw.premod)),
                    ("panic_trips", Value::Bool(sw.panic_trips)),
                ]),
            ),
            ("efforts", cbor::map(vec![("reply", Value::Uint(u64::from(self.efforts.reply))), ("thread", Value::Uint(u64::from(self.efforts.thread)))])),
        ])
        .encode()
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let v = Value::decode(b)?;
        if v.get("v")?.uint()? != 1 {
            return None;
        }
        let known = read_keys(&v, "known", KNOWN)?.into();
        let held_v = v.get("held")?.array()?;
        if held_v.len() > HELD {
            return None;
        }
        let held = held_v
            .iter()
            .map(|h| {
                let s = Signed::from_value(h.get("s")?).ok()?;
                let c = u8::try_from(h.get("cap")?.uint()?).ok().filter(|c| *c <= cap::OWNER)?;
                Some(Held { s, sig: h.get("sig")?.bytes()?.try_into().ok()?, cap: c, at: h.get("at")?.uint()? })
            })
            .collect::<Option<Vec<_>>>()?;
        let sw = v.get("sw")?;
        let f = |k: &str| sw.get(k).and_then(Value::boolean);
        let e = v.get("efforts")?;
        let u = |k: &str| e.get(k).and_then(Value::uint).and_then(|x| u32::try_from(x).ok()).filter(|x| *x > 0);
        Some(Own {
            bans: read_keys(&v, "bans", BANS)?,
            approved: read_keys(&v, "approved", APPROVED)?,
            known,
            held,
            sw: Switches { paused: f("paused")?, threads_closed: f("threads_closed")?, trips_only: f("trips_only")?, approved_only: f("approved_only")?, premod: f("premod")?, panic_trips: f("panic_trips")? },
            efforts: Efforts { reply: u("reply")?, thread: u("thread")? },
        })
    }

    /// The sealed block for board key `pk`, with a fresh `nonce`; `None` if it would exceed
    /// [`limits::OWN`] (the caps above keep it well under).
    pub fn seal(&self, key: &[u8; 32], pk: &[u8; 32], nonce: &[u8; NONCE]) -> Option<Vec<u8>> {
        let body = self.encode();
        let total = (NONCE + TAG + 4 + body.len()).div_ceil(STEP) * STEP;
        if total > limits::OWN {
            return None;
        }
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(nonce);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out.resize(total - TAG, 0);
        let mut aad = [0u8; AAD.len() + 32];
        aad[..AAD.len()].copy_from_slice(AAD);
        aad[AAD.len()..].copy_from_slice(pk);
        let tag = XChaCha20Poly1305::new(key.into()).encrypt_in_place_detached(XNonce::from_slice(nonce), &aad, &mut out[NONCE..]).ok()?;
        out.extend_from_slice(&tag);
        Some(out)
    }

    pub fn open(key: &[u8; 32], pk: &[u8; 32], sealed: &[u8]) -> Option<Self> {
        if sealed.len() < NONCE + TAG + 4 || sealed.len() > limits::OWN || !sealed.len().is_multiple_of(STEP) {
            return None;
        }
        let (nonce, rest) = sealed.split_at(NONCE);
        let (ct, tag) = rest.split_at(rest.len() - TAG);
        let mut buf = ct.to_vec();
        let mut aad = [0u8; AAD.len() + 32];
        aad[..AAD.len()].copy_from_slice(AAD);
        aad[AAD.len()..].copy_from_slice(pk);
        XChaCha20Poly1305::new(key.into()).decrypt_in_place_detached(XNonce::from_slice(nonce), &aad, &mut buf, Tag::from_slice(tag)).ok()?;
        let len = u32::from_le_bytes(buf[..4].try_into().ok()?) as usize;
        Self::decode(buf.get(4..4 + len)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Own {
        let mut o = Own::default();
        o.bans.push([1; 32]);
        o.approved.push([2; 32]);
        o.saw_trip(&[3; 32]);
        o.sw.premod = true;
        o.efforts = Efforts { reply: 9, thread: 99 };
        let s = Signed { b: "k51x".into(), t: 4, k: [5; 32], n: [6; 16], sub: String::new(), body: "held".into(), sage: false, e: 7, trip: true };
        o.held.push(Held { s, sig: [8; 64], cap: 0, at: 1_790_000_000 });
        o
    }

    #[test]
    fn seal_and_open() {
        let o = sample();
        let sealed = o.seal(&[9; 32], &[10; 32], &[11; NONCE]).unwrap();
        assert_eq!(sealed.len() % STEP, 0, "padded to 4 KiB steps");
        assert_eq!(Own::open(&[9; 32], &[10; 32], &sealed), Some(o));
        assert_eq!(Own::open(&[8; 32], &[10; 32], &sealed), None, "another key");
        assert_eq!(Own::open(&[9; 32], &[12; 32], &sealed), None, "another board");
        assert_eq!(Own::default().seal(&[9; 32], &[10; 32], &[0; NONCE]).unwrap().len(), sealed.len(), "the size says nothing about a small state");
    }

    #[test]
    fn full_state_fits() {
        let mut o = sample();
        o.bans = vec![[1; 32]; BANS];
        o.approved = vec![[2; 32]; APPROVED];
        for i in 0..KNOWN + 10 {
            o.saw_trip(&[(i % 251) as u8, (i / 251) as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        assert_eq!(o.known.len(), KNOWN);
        let h = o.held[0].clone();
        o.held = vec![Held { s: Signed { body: "x".repeat(limits::BODY), ..h.s }, ..h }; HELD];
        let sealed = o.seal(&[9; 32], &[10; 32], &[11; NONCE]).expect("within the 64 KiB cap");
        assert_eq!(Own::open(&[9; 32], &[10; 32], &sealed), Some(o));
    }
}
