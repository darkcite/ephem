// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// S5b: Argon2id cost in WASM under V8 (the engine Chrome uses).
import * as fs from 'node:fs';
const wasm = fs.readFileSync(process.argv[2]);
const inst = new WebAssembly.Instance(new WebAssembly.Module(wasm), {});
inst.exports.kdf(1024, 1);
for (const [m, t] of [[19456, 2], [19456, 4], [65536, 3]]) {
  const t0 = performance.now(); inst.exports.kdf(m, t);
  console.log(`| Argon2id m=${m} KiB t=${t} | ${Math.round(performance.now() - t0)} ms | wasm memory ${inst.exports.memory.buffer.byteLength >> 20} MiB |`);
}
