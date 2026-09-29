// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P1: Equi-X in headless Chromium (wasm32, interpreted hashx).
// Build first:  cargo build --release --lib --target wasm32-unknown-unknown &&
//   wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/equix_bench.wasm
// Run:  node run.mjs [K=200] [workers=4]   (from checks/spikes/equix_bench)
import * as fs from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as url from 'node:url';
import { launch } from '../../e2e_lib.mjs';

const DIR = path.dirname(url.fileURLToPath(import.meta.url));
const K = Number(process.argv[2] || 200), W = Number(process.argv[3] || 4);
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const srv = http.createServer((q, r) => {
  const p = path.join(DIR, path.normalize(decodeURIComponent(new URL(q.url, 'http://x').pathname)));
  if (!p.startsWith(DIR + path.sep) || !fs.existsSync(p) || !fs.statSync(p).isFile()) { r.writeHead(404); r.end(); return; }
  r.writeHead(200, { 'content-type': TYPES[path.extname(p)] || 'application/octet-stream' });
  fs.createReadStream(p).pipe(r);
});
await new Promise((res) => srv.listen(0, '127.0.0.1', res));
const browser = await launch();
const page = await browser.newPage();
page.on('console', (m) => console.log('[page]', m.text()));
page.on('pageerror', (e) => console.log('[pageerror]', e.message));
await page.goto(`http://127.0.0.1:${srv.address().port}/bench.html`);
await page.waitForFunction(() => window.ready);
console.log('browser', browser.version(), '| hardwareConcurrency', await page.evaluate(() => navigator.hardwareConcurrency));

function stats(name, v) {
  v = [...v].sort((a, b) => a - b);
  const q = (x) => v[Math.min(v.length - 1, Math.floor(v.length * x))];
  const mean = v.reduce((a, b) => a + b, 0) / v.length;
  console.log(`${name.padEnd(36)} n=${String(v.length).padEnd(4)} mean=${mean.toFixed(3).padStart(8)} ms  median=${q(0.5).toFixed(3).padStart(8)}  p95=${q(0.95).toFixed(3).padStart(8)}  min=${v[0].toFixed(3)}  max=${v[v.length - 1].toFixed(3)}`);
  return { mean, median: q(0.5), p95: q(0.95) };
}

await page.evaluate(() => window.runMain(5, 1)); // warm-up (wasm compile/tier-up)
const r = await page.evaluate(([k]) => window.runMain(k, 1_000_000), [K]);
console.log('--- main thread, 1 core ---');
stats('hashx build only', r.build);
const s = stats('attempt (build + solve)', r.solve);
stats('verify one solution (build incl.)', r.verify);
const spa = r.sols / r.solve.length;
console.log(`solutions/attempt = ${spa.toFixed(3)} (${r.sols}/${r.solve.length}); skipped = ${r.skipped}; ` +
  `${(1000 / s.mean).toFixed(2)} attempts/s, ${(1000 * spa / s.mean).toFixed(2)} solutions/s per core`);

for (const w of [...new Set([2, W])]) {
  const per = Math.max(10, Math.floor(K / 2));
  const m = await page.evaluate(([w, k]) => window.runWorkers(w, k), [w, per]);
  const att = m.out.reduce((a, o) => a + o.solve.length, 0), sols = m.out.reduce((a, o) => a + o.sols, 0);
  const all = m.out.flatMap((o) => o.solve);
  console.log(`--- ${w} Web Workers x ${per} attempts ---`);
  stats('attempt per worker', all);
  console.log(`aggregate: ${att} attempts in ${(m.wall / 1000).toFixed(2)} s wall = ${(att * 1000 / m.wall).toFixed(2)} attempts/s, ${(sols * 1000 / m.wall).toFixed(2)} solutions/s`);
}
await browser.close();
srv.close();
