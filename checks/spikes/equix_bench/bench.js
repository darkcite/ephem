// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P1: times Equi-X attempts of pkg/equix_bench.js (wasm32, interpreted hashx).
// Used both on the page's main thread and inside module Web Workers.
import init, { init as initMem, solve_one, build_one, verify_last } from './pkg/equix_bench.js';

export async function bench(k, base) {
  await init();
  initMem();
  const build = [], solve = [], verify = [];
  let sols = 0, skipped = 0, i = base;
  const t0 = performance.now();
  while (solve.length < k) {
    let t = performance.now();
    build_one(i);
    build.push(performance.now() - t);
    t = performance.now();
    const n = solve_one(i);
    const dt = performance.now() - t;
    i++;
    if (n < 0) { skipped++; continue; }
    solve.push(dt); // includes the hashx program build (as one real attempt does)
    sols += n;
    if (n > 0) {
      t = performance.now();
      const ok = verify_last();
      verify.push((performance.now() - t) / n);
      if (ok !== n) throw new Error(`verify ${ok}/${n}`);
    }
  }
  return { build, solve, verify, sols, skipped, wall: performance.now() - t0 };
}

// In a worker: run on the first message and post the result back.
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  self.onmessage = async (e) => {
    const { k, base } = e.data;
    const t0 = performance.now();
    const r = await bench(k, base);
    self.postMessage({ ...r, start: t0, end: performance.now() });
  };
}
