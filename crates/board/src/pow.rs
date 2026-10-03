// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The proof of work of a submission (G.8): Equi-X (Tor's onion-service PoW v1 puzzle) with
//! Tor's effort predicate, bound to the post key and a per-post nonce instead of the content,
//! so a poster solves while typing.
//!
//! `challenge = "ephem-board-pow-v2" ‖ board name ‖ seed(epoch) ‖ kind ‖ thread ‖ k ‖ n ‖ effort`;
//! a solution is valid when Equi-X accepts it and `BLAKE2b-32(challenge ‖ solution) × effort ≤
//! 2³² − 1`. The expected number of solutions to find is `effort`. The poster retries by drawing
//! a new `n` (it is random anyway and goes into the signed post).

use blake2::digest::consts::U32;
use blake2::digest::{Mac, Update, VariableOutput};
use blake2::{Blake2bMac, Blake2bVar};
use equix::{EquiXBuilder, RuntimeOption, Solution, SolverMemory};

pub const TAG: &[u8] = b"ephem-board-pow-v2";
/// Longest board name in bytes (a CIDv1 of a libp2p-key Ed25519 identity is 40).
pub const MAX_NAME: usize = 48;
pub const SOLUTION_LEN: usize = Solution::NUM_BYTES;
/// Epoch length: seeds change every 10 minutes; the current and previous epochs are accepted.
pub const EPOCH_S: u64 = 600;

/// The challenge bytes, built in place (no allocation).
#[derive(Clone, Copy)]
pub struct Challenge {
    buf: [u8; TAG.len() + MAX_NAME + 32 + 1 + 8 + 32 + 16 + 4],
    len: usize,
}

impl Challenge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(name: &[u8], seed: &[u8; 32], kind: u8, thread: u64, k: &[u8; 32], n: &[u8; 16], effort: u32) -> Option<Self> {
        if name.len() > MAX_NAME {
            return None;
        }
        let mut c = Challenge { buf: [0; TAG.len() + MAX_NAME + 32 + 1 + 8 + 32 + 16 + 4], len: 0 };
        for part in [TAG, name, seed, &[kind], &thread.to_le_bytes(), k, n, &effort.to_le_bytes()] {
            c.buf[c.len..c.len + part.len()].copy_from_slice(part);
            c.len += part.len();
        }
        Some(c)
    }

    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Replaces the nonce (the poster's retry), keeping everything else.
    pub fn set_nonce(&mut self, n: &[u8; 16]) {
        let at = self.len - 4 - 16;
        self.buf[at..at + 16].copy_from_slice(n);
    }
}

/// Tor's predicate: `BLAKE2b-32(challenge ‖ solution) × effort ≤ 2³² − 1`.
#[inline]
fn meets_effort(c: &Challenge, solution: &[u8; SOLUTION_LEN], effort: u32) -> bool {
    let mut h = Blake2bVar::new(4).expect("4-byte output is valid");
    h.update(c.bytes());
    h.update(solution);
    let mut out = [0u8; 4];
    h.finalize_variable(&mut out).expect("4-byte output");
    u64::from(u32::from_be_bytes(out)) * u64::from(effort) <= u64::from(u32::MAX)
}

fn builder() -> EquiXBuilder {
    let mut b = EquiXBuilder::new();
    b.runtime(RuntimeOption::TryCompile);
    b
}

/// The host's check (≈ 0.1 ms): the effort predicate first (one hash), then Equi-X.
pub fn verify(c: &Challenge, solution: &[u8; SOLUTION_LEN], effort: u32) -> bool {
    meets_effort(c, solution, effort) && builder().verify_bytes(c.bytes(), solution).is_ok()
}

/// One attempt for the current nonce: a solution meeting `effort`, if any. The caller draws a
/// new nonce between attempts (`Challenge::set_nonce`); `mem` (≈ 1.8 MB) is reused.
pub fn attempt(c: &Challenge, effort: u32, mem: &mut SolverMemory) -> Option<[u8; SOLUTION_LEN]> {
    let e = builder().build(c.bytes()).ok()?; // some programs are rejected; draw another nonce
    for s in e.solve_with_memory(mem).iter() {
        let b = s.to_bytes();
        if meets_effort(c, &b, effort) {
            return Some(b);
        }
    }
    None
}

/// `seed(epoch) = BLAKE2b-keyed(pow secret, "ephem-board-seed-v1" ‖ epoch)`: unpredictable before
/// its epoch, the same on every device of the owner (the secret is derived from the identity).
pub fn seed(secret: &[u8; 32], epoch: u32) -> [u8; 32] {
    let mut m = <Blake2bMac<U32> as Mac>::new_from_slice(secret).expect("a 32-byte key is valid");
    Mac::update(&mut m, b"ephem-board-seed-v1");
    Mac::update(&mut m, &epoch.to_le_bytes());
    m.finalize().into_bytes().into()
}

/// The epoch of `now_s`.
#[inline]
pub fn epoch(now_s: u64) -> u32 {
    (now_s / EPOCH_S) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solve_and_verify_bound_to_everything() {
        let seed = seed(&[7; 32], 1);
        let mut c = Challenge::new(b"name", &seed, 2, 5, &[1; 32], &[0; 16], 1).unwrap();
        let mut mem = SolverMemory::new();
        let mut n = [0u8; 16];
        let sol = loop {
            if let Some(s) = attempt(&c, 1, &mut mem) {
                break s;
            }
            n[0] += 1;
            c.set_nonce(&n);
        };
        assert!(verify(&c, &sol, 1));
        // Any other thread, key, nonce, kind or seed: the solution does not verify.
        for other in [
            Challenge::new(b"name", &seed, 2, 6, &[1; 32], &n, 1),
            Challenge::new(b"name", &seed, 2, 5, &[2; 32], &n, 1),
            Challenge::new(b"name", &seed, 1, 5, &[1; 32], &n, 1),
            Challenge::new(b"name", &super::seed(&[7; 32], 2), 2, 5, &[1; 32], &n, 1),
            Challenge::new(b"other", &seed, 2, 5, &[1; 32], &n, 1),
        ] {
            assert!(!verify(&other.unwrap(), &sol, 1));
        }
    }
}
