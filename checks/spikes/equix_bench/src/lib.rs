// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Spike B-P1 (docs/BOARDS.md G.16): Equi-X solve/verify timing, shared by the native binary
//! and the wasm32 build (timed from JS with performance.now()).

use equix::{EquiX, EquiXBuilder, RuntimeOption, SolutionArray, SolverMemory};

/// A 100-byte challenge, the size of Tor's HS PoW v1 challenge
/// ("Tor hs intro v1\0" 16 + blinded id 32 + seed 32 + nonce 16 + effort 4).
pub fn challenge(i: u64) -> [u8; 100] {
    let mut c = [0u8; 100];
    c[..16].copy_from_slice(b"Tor hs intro v1\0");
    for (k, b) in c[16..80].iter_mut().enumerate() {
        *b = (k as u8).wrapping_mul(37).wrapping_add(11);
    }
    c[80..88].copy_from_slice(&i.to_le_bytes()); // nonce
    c[96..100].copy_from_slice(&1u32.to_be_bytes()); // effort
    c
}

pub fn builder(interpret_only: bool) -> EquiXBuilder {
    let mut b = EquiXBuilder::new();
    b.runtime(if interpret_only { RuntimeOption::InterpretOnly } else { RuntimeOption::TryCompile });
    b
}

/// Builds the hashx program for challenge `i` (None on ProgramConstraints: Tor skips such nonces).
pub fn build(b: &EquiXBuilder, i: u64) -> Option<EquiX> {
    b.build(&challenge(i)).ok()
}

/// One solve attempt with reused memory. None when the challenge is skipped.
pub fn attempt(b: &EquiXBuilder, mem: &mut SolverMemory, i: u64) -> Option<SolutionArray> {
    build(b, i).map(|e| e.solve_with_memory(mem))
}

/// Full verification of every solution (hashx program build included, as a verifier does).
pub fn verify_all(b: &EquiXBuilder, i: u64, sols: &SolutionArray) -> usize {
    let c = challenge(i);
    sols.iter().filter(|s| b.verify(&c, s).is_ok()).count()
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static MEM: RefCell<Option<Box<SolverMemory>>> = const { RefCell::new(None) };
        static LAST: RefCell<(u64, SolutionArray)> = RefCell::new((0, SolutionArray::new()));
    }

    /// Allocates the solver memory once (~1.8 MB), so solve timing excludes it.
    #[wasm_bindgen]
    pub fn init() {
        MEM.with(|m| *m.borrow_mut() = Some(Box::new(SolverMemory::new())));
    }

    /// One Equi-X attempt for nonce `i` (interpreted hashx: no JIT in wasm).
    /// Returns the number of solutions, or -1 if the challenge is skipped.
    #[wasm_bindgen]
    pub fn solve_one(i: u32) -> i32 {
        let b = builder(true);
        MEM.with(|m| {
            let mut m = m.borrow_mut();
            let mem = m.as_mut().expect("init first");
            match attempt(&b, mem, i as u64) {
                Some(s) => {
                    let n = s.len() as i32;
                    LAST.with(|l| *l.borrow_mut() = (i as u64, s));
                    n
                }
                None => -1,
            }
        })
    }

    /// Only the hashx program build for nonce `i` (1 if ok, 0 if skipped).
    #[wasm_bindgen]
    pub fn build_one(i: u32) -> u32 {
        build(&builder(true), i as u64).is_some() as u32
    }

    /// Verifies every solution of the last solve (program build included per solution).
    /// Returns how many verified.
    #[wasm_bindgen]
    pub fn verify_last() -> u32 {
        LAST.with(|l| {
            let l = l.borrow();
            verify_all(&builder(true), l.0, &l.1) as u32
        })
    }
}
