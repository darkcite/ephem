// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The boards' proof of work in a Web Worker (docs/BOARDS.md G.6.1 step 2, G.8): the poster's
//! tab solves while the user types, in 1–4 Workers. A raw wasm ABI with no imports and no
//! JavaScript glue: the page compiles `ephem_pow.wasm` once (fetched with its SHA-384), posts the
//! `WebAssembly.Module` to each same-origin Worker, and the Worker calls two exports:
//!
//! - `buf() -> *mut u8`: the fixed 256-byte exchange buffer ([`layout`]);
//! - `solve(name_len, kind, effort, attempts) -> u32`: tries up to `attempts` nonces from the
//!   one in the buffer; 1 = solved (the nonce and the solution are in the buffer), 0 = not yet
//!   (the buffer holds the next nonce to try).
//!
//! The Equi-X solver memory (≈ 1.8 MB) is allocated on the first call and reused.

use ephem_board::pow::{self, Challenge, MAX_NAME, SOLUTION_LEN};
use equix::SolverMemory;
use std::cell::UnsafeCell;

/// Offsets in the exchange buffer.
pub mod layout {
    use super::*;
    /// The board's name (binary CID), `name_len` bytes.
    pub const NAME: usize = 0;
    pub const SEED: usize = NAME + MAX_NAME;
    /// `thread`, u64 little-endian.
    pub const THREAD: usize = SEED + 32;
    pub const K: usize = THREAD + 8;
    /// In: the first nonce. Out: the solving (or next) nonce.
    pub const N: usize = K + 32;
    /// Out: the solution.
    pub const SOLUTION: usize = N + 16;
    pub const LEN: usize = 256;
    const _: () = assert!(SOLUTION + SOLUTION_LEN <= LEN);
}

struct State {
    buf: [u8; layout::LEN],
    mem: Option<Box<SolverMemory>>,
}

/// One Worker, one thread: the cell is never shared.
struct Cell(UnsafeCell<State>);
unsafe impl Sync for Cell {}
static STATE: Cell = Cell(UnsafeCell::new(State { buf: [0; layout::LEN], mem: None }));

#[unsafe(no_mangle)]
pub extern "C" fn buf() -> *mut u8 {
    // SAFETY: wasm Workers are single-threaded; the pointer stays valid (a static).
    unsafe { (*STATE.0.get()).buf.as_mut_ptr() }
}

#[unsafe(no_mangle)]
pub extern "C" fn solve(name_len: u32, kind: u32, effort: u32, attempts: u32) -> u32 {
    // SAFETY: single-threaded (see `buf`); no other reference to the state is live here.
    let st = unsafe { &mut *STATE.0.get() };
    solve_in(st, name_len as usize, kind as u8, effort, attempts)
}

fn arr<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b[at..at + N].try_into().expect("in the buffer")
}

fn solve_in(st: &mut State, name_len: usize, kind: u8, effort: u32, attempts: u32) -> u32 {
    if name_len > MAX_NAME || effort == 0 {
        return 0;
    }
    let b = &st.buf;
    let (seed, k, mut n) = (arr::<32>(b, layout::SEED), arr::<32>(b, layout::K), arr::<16>(b, layout::N));
    let thread = u64::from_le_bytes(arr(b, layout::THREAD));
    let Some(mut c) = Challenge::new(&b[layout::NAME..layout::NAME + name_len], &seed, kind, thread, &k, &n, effort) else { return 0 };
    let mem = st.mem.get_or_insert_with(|| Box::new(SolverMemory::new()));
    let found = pow::solve(&mut c, &mut n, effort, mem, attempts);
    st.buf[layout::N..layout::N + 16].copy_from_slice(&n);
    match found {
        Some(s) => {
            st.buf[layout::SOLUTION..layout::SOLUTION + SOLUTION_LEN].copy_from_slice(&s);
            1
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_what_the_host_accepts() {
        let st = unsafe { &mut *STATE.0.get() };
        let name = [7u8; 38];
        st.buf[..38].copy_from_slice(&name);
        st.buf[layout::SEED..layout::SEED + 32].fill(1);
        st.buf[layout::THREAD..layout::THREAD + 8].copy_from_slice(&5u64.to_le_bytes());
        st.buf[layout::K..layout::K + 32].fill(2);
        st.buf[layout::N..layout::N + 16].fill(3);
        let effort = 4;
        let mut tries = 0;
        while solve(38, 2, effort, 1) == 0 {
            tries += 1;
            assert!(tries < 1_000);
        }
        let n = arr::<16>(&st.buf, layout::N);
        let c = Challenge::new(&name, &[1; 32], 2, 5, &[2; 32], &n, effort).unwrap();
        assert!(pow::verify(&c, &arr(&st.buf, layout::SOLUTION), effort));
    }
}
