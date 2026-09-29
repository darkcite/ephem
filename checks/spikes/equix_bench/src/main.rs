// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Spike B-P1 native bench: `cargo run --release -- [N] [threads]`.

use equix::SolverMemory;
use equix_bench::*;
use std::time::Instant;

fn stats(name: &str, v: &mut [f64]) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let p = |q: f64| v[((n as f64 * q) as usize).min(n - 1)];
    println!(
        "{name:<34} n={n:<4} mean={mean:8.3} ms  median={:8.3}  p95={:8.3}  min={:8.3}  max={:8.3}",
        p(0.5), p(0.95), v[0], v[n - 1]
    );
}

fn run(interpret: bool, n: u64, base: u64) {
    let b = builder(interpret);
    let rt = build(&b, base).map(|e| format!("{:?}", e.runtime())).unwrap_or_default();
    println!("--- runtime option {} (effective: {rt}) ---", if interpret { "InterpretOnly" } else { "TryCompile" });
    let mut mem = SolverMemory::new();
    let (mut tb, mut ts, mut tv, mut tf) = (vec![], vec![], vec![], vec![]);
    let (mut sols, mut skipped, mut tries) = (0usize, 0usize, 0u64);
    let mut i = base;
    while (ts.len() as u64) < n {
        tries += 1;
        let t = Instant::now();
        let e = build(&b, i);
        tb.push(t.elapsed().as_secs_f64() * 1e3);
        let Some(e) = e else { skipped += 1; i += 1; continue };
        let t = Instant::now();
        let s = e.solve_with_memory(&mut mem);
        ts.push(t.elapsed().as_secs_f64() * 1e3);
        // Fresh-memory variant (equix::solve allocates SolverMemory each call).
        let t = Instant::now();
        let s2 = b.solve(&equix_bench::challenge(i)).unwrap();
        tf.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(s.len(), s2.len());
        sols += s.len();
        for sol in s.iter() {
            let t = Instant::now();
            assert!(b.verify(&equix_bench::challenge(i), sol).is_ok());
            tv.push(t.elapsed().as_secs_f64() * 1e3);
        }
        i += 1;
    }
    stats("hashx build (per challenge)", &mut tb);
    stats("solve_with_memory (build excl.)", &mut ts);
    stats("equix::solve-like (build+alloc)", &mut tf);
    stats("verify one solution (build incl.)", &mut tv);
    println!(
        "solutions/attempt = {:.3} ({sols}/{n}); skipped challenges = {skipped}/{tries}",
        sols as f64 / n as f64
    );
}

fn main() {
    let args: Vec<u64> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let n = *args.first().unwrap_or(&200);
    let threads = *args.get(1).unwrap_or(&4);
    run(false, n, 1_000_000);
    run(true, n, 1_000_000);
    // Throughput with `threads` threads, interpreted and compiled.
    for interpret in [false, true] {
        let per = n / 2;
        let t = Instant::now();
        let hs: Vec<_> = (0..threads)
            .map(|k| {
                std::thread::spawn(move || {
                    let b = builder(interpret);
                    let mut mem = SolverMemory::new();
                    let mut sols = 0;
                    for j in 0..per {
                        if let Some(s) = attempt(&b, &mut mem, 5_000_000 + k * 100_000 + j) {
                            sols += s.len();
                        }
                    }
                    sols
                })
            })
            .collect();
        let sols: usize = hs.into_iter().map(|h| h.join().unwrap()).sum();
        let secs = t.elapsed().as_secs_f64();
        println!(
            "{threads} threads, {}: {} attempts in {secs:.2} s = {:.1} attempts/s, {:.1} solutions/s",
            if interpret { "interpret" } else { "compiled" },
            threads * per,
            (threads * per) as f64 / secs,
            sols as f64 / secs
        );
    }
}
