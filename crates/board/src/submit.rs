// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The submit format (G.6.1): a fixed 188-byte header, then `s` (dag-cbor) and, in v2, image
//! bytes. Everything needed to refuse a submission is in the header, so the host checks the
//! proof of work before reading a single body byte. Little-endian, parsed in place.
//!
//! | Off | Size | Field |
//! |---|---|---|
//! | 0 | 4 | `magic` `EPB2` |
//! | 4 | 1 | `kind` |
//! | 5 | 1 | `flags` (bit0 sage; others 0 in v1) |
//! | 6 | 2 | `text_len` |
//! | 8 | 4 | `img_len` (0 in v1) |
//! | 12 | 4 | `epoch` |
//! | 16 | 4 | `effort` |
//! | 20 | 8 | `thread` |
//! | 28 | 32 | `k` |
//! | 60 | 16 | `n` |
//! | 76 | 32 | `h` = BLAKE2b-256(s ‖ image) |
//! | 108 | 16 | `solution` |
//! | 124 | 64 | `sig` |

use crate::pow::SOLUTION_LEN;
use blake2::Blake2b;
use blake2::digest::{Digest, consts::U32};

pub const MAGIC: &[u8; 4] = b"EPB2";
pub const HEADER_LEN: usize = 188;
/// `s` (the action map for owner actions), dag-cbor.
pub const MAX_TEXT: usize = 2_400;
/// v1 is text only (an image is refused at the header); v2 raises this to 512 KiB.
pub const MAX_IMAGE: u32 = 0;
pub const MAX_SUBMIT: usize = HEADER_LEN + MAX_TEXT;

/// Submission kinds (`kind`).
pub mod kind {
    pub const THREAD: u8 = 1;
    pub const REPLY: u8 = 2;
    pub const SELF_DELETE: u8 = 3;
    pub const REPORT: u8 = 4;
    pub const ACTION: u8 = 5;
    pub const CAPCODE: u8 = 6;
}

pub mod flag {
    pub const SAGE: u8 = 1 << 0;
    pub const KNOWN: u8 = SAGE;
}

/// A parsed header: plain values, copied out of the 188 bytes (no allocation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub flags: u8,
    pub text_len: u16,
    pub img_len: u32,
    pub epoch: u32,
    pub effort: u32,
    pub thread: u64,
    pub k: [u8; 32],
    pub n: [u8; 16],
    pub h: [u8; 32],
    pub solution: [u8; SOLUTION_LEN],
    pub sig: [u8; 64],
}

#[inline]
fn arr<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    let mut a = [0u8; N];
    a.copy_from_slice(&b[at..at + N]);
    a
}

impl Header {
    /// The fixed header, or `None` (bad magic, unknown kind or flags, impossible lengths).
    pub fn parse(b: &[u8; HEADER_LEN]) -> Option<Self> {
        if &b[0..4] != MAGIC {
            return None;
        }
        let h = Header {
            kind: b[4],
            flags: b[5],
            text_len: u16::from_le_bytes(arr(b, 6)),
            img_len: u32::from_le_bytes(arr(b, 8)),
            epoch: u32::from_le_bytes(arr(b, 12)),
            effort: u32::from_le_bytes(arr(b, 16)),
            thread: u64::from_le_bytes(arr(b, 20)),
            k: arr(b, 28),
            n: arr(b, 60),
            h: arr(b, 76),
            solution: arr(b, 108),
            sig: arr(b, 124),
        };
        let ok = (kind::THREAD..=kind::CAPCODE).contains(&h.kind)
            && h.flags & !flag::KNOWN == 0
            && (1..=MAX_TEXT as u16).contains(&h.text_len)
            && h.img_len == 0 // v1: text only (MAX_IMAGE is 0)
            && (h.kind == kind::THREAD) == (h.thread == 0);
        ok.then_some(h)
    }

    /// The whole request's length (header + body), what `Content-Length` must say.
    #[inline]
    pub fn total_len(&self) -> usize {
        HEADER_LEN + self.text_len as usize + self.img_len as usize
    }

    pub fn write(&self, out: &mut [u8; HEADER_LEN]) {
        out[0..4].copy_from_slice(MAGIC);
        out[4] = self.kind;
        out[5] = self.flags;
        out[6..8].copy_from_slice(&self.text_len.to_le_bytes());
        out[8..12].copy_from_slice(&self.img_len.to_le_bytes());
        out[12..16].copy_from_slice(&self.epoch.to_le_bytes());
        out[16..20].copy_from_slice(&self.effort.to_le_bytes());
        out[20..28].copy_from_slice(&self.thread.to_le_bytes());
        out[28..60].copy_from_slice(&self.k);
        out[60..76].copy_from_slice(&self.n);
        out[76..108].copy_from_slice(&self.h);
        out[108..124].copy_from_slice(&self.solution);
        out[124..188].copy_from_slice(&self.sig);
    }
}

/// `h` of a body (`s ‖ image`).
pub fn body_hash(body: &[u8]) -> [u8; 32] {
    Blake2b::<U32>::digest(body).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header { kind: kind::REPLY, flags: flag::SAGE, text_len: 100, img_len: 0, epoch: 9, effort: 300, thread: 42, k: [1; 32], n: [2; 16], h: [3; 32], solution: [4; SOLUTION_LEN], sig: [5; 64] }
    }

    #[test]
    fn round_trip_and_refusals() {
        let mut b = [0u8; HEADER_LEN];
        sample().write(&mut b);
        assert_eq!(Header::parse(&b), Some(sample()));
        let refuse = |f: &dyn Fn(&mut [u8; HEADER_LEN])| {
            let mut x = b;
            f(&mut x);
            Header::parse(&x).is_none()
        };
        assert!(refuse(&|x| x[0] = b'X'), "magic");
        assert!(refuse(&|x| x[4] = 9), "kind");
        assert!(refuse(&|x| x[5] = 0x80), "unknown flag");
        assert!(refuse(&|x| x[6..8].copy_from_slice(&0u16.to_le_bytes())), "empty body");
        assert!(refuse(&|x| x[6..8].copy_from_slice(&(MAX_TEXT as u16 + 1).to_le_bytes())), "text too long");
        assert!(refuse(&|x| x[8] = 1), "an image in v1");
        assert!(refuse(&|x| x[20..28].copy_from_slice(&0u64.to_le_bytes())), "a reply to thread 0");
        assert!(refuse(&|x| x[4] = kind::THREAD), "a new thread naming a thread");
    }
}
