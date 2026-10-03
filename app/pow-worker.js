// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// One Web Worker of the boards' proof of work (docs/BOARDS.md G.6.1 step 2): the page posts the
// compiled ephem_pow.wasm (fetched with its SHA-384, crates/pow) and one challenge; this worker
// solves until it finds a solution or the page terminates it. No imports, no other code.
const L = { NAME: 0, SEED: 48, THREAD: 80, K: 88, N: 120, SOLUTION: 136 };

self.onmessage = async (e) => {
  try {
    const { module, name, seed, thread, k, n, kind, effort } = e.data;
    const ex = (await WebAssembly.instantiate(module, {})).exports;
    // A fresh view each time: the first solve grows the memory (the solver's ~1.8 MB), which
    // detaches views of the old buffer.
    const view = () => new Uint8Array(ex.memory.buffer, ex.buf(), 256);
    const buf = view();
    buf.set(name, L.NAME);
    buf.set(seed, L.SEED);
    new DataView(buf.buffer, buf.byteOffset).setBigUint64(L.THREAD, BigInt(thread), true);
    buf.set(k, L.K);
    buf.set(n, L.N);
    let attempts = 0;
    // A few attempts per call (~0.1 s each), then report progress and go on.
    while (ex.solve(name.length, kind, effort, 4) === 0) {
      attempts += 4;
      self.postMessage({ attempts });
    }
    const out = view();
    self.postMessage({ n: out.slice(L.N, L.N + 16), solution: out.slice(L.SOLUTION, L.SOLUTION + 16) });
  } catch (err) {
    self.postMessage({ error: String(err?.message || err) });
  }
};
