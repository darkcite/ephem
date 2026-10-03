// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Security audit H-1: a block of nested arrays or maps that each declare as many items as bytes
//! remain must not reserve memory for the declared lengths. Before the fix, a 1 MiB block peaked
//! at 839 MiB (and a 4 MiB block exceeds the wasm32 limit, aborting the tab).
//! Blocks are capped at 1 MiB (`car::MAX_BLOCK`), so a decode now stays under ~64 MiB.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ephem_channel::car;
use ephem_channel::cbor::Value;
use ephem_channel::cid::{Cid, DAG_CBOR};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The counters are process-wide: tests that allocate run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// `levels` nested containers (major 4 = array, 5 = map), each claiming every remaining byte as
/// items, padded to `size` with `0x00` (uint 0).
fn bomb(major: u8, levels: usize, size: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(size);
    for i in 0..levels {
        let rest = (size - (i + 1) * 5) as u32;
        b.push(major << 5 | 26);
        b.extend_from_slice(&rest.to_be_bytes());
    }
    b.resize(size, 0);
    b
}

fn peak_during(f: impl FnOnce()) -> usize {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    f();
    PEAK.load(Ordering::Relaxed) - base
}

#[test]
fn nested_lengths_do_not_reserve_memory() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for major in [4u8, 5] {
        let block = bomb(major, 15, 1 << 20);
        let peak = peak_during(|| assert!(Value::decode(&block).is_none()));
        // What remains is the values actually parsed: ≤ one 32-byte `Value` per input byte, and
        // a vector's doubling (≤ 2×). Before the fix: 480–840 MiB.
        assert!(peak < 80 << 20, "major {major}: a 1 MiB block peaked at {} KiB", peak >> 10);
    }
}

#[test]
fn honest_arrays_still_decode() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let v = Value::Array((0..10_000).map(Value::Uint).collect());
    assert_eq!(Value::decode(&v.encode()), Some(v));
}

#[test]
fn car_refuses_oversized_blocks() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let big = vec![0x40u8; car::MAX_BLOCK + 1]; // not valid CBOR, but the size check comes first
    let cid = Cid::of(DAG_CBOR, &big);
    let file = car::write(std::slice::from_ref(&cid), &[(cid.clone(), big)]);
    assert!(car::read(&file).is_none());
    let ok = Value::Uint(1).encode();
    let cid = Cid::of(DAG_CBOR, &ok);
    assert!(car::read(&car::write(std::slice::from_ref(&cid), &[(cid.clone(), ok)])).is_some());
}
