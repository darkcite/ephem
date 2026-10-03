// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P6 (docs/BOARDS.md G.16.2), the store part, in Chromium: a full board's blocks (1 368
// files, ~18.7 MB, as `crates/board/tests/model.rs` builds) written through the app's own
// store Worker (app/store-worker.js, SyncAccessHandle), a publish-sized update (25 files written,
// 25 deleted), then a startup read of every file; and the main thread's createWritable for 200
// files, for comparison. `node checks/spikes/board_store/run.mjs`.
import { launch, serve } from '../../e2e_lib.mjs';

const srv = await serve();
const b = await launch();
const p = await b.newPage();
await p.goto(`http://127.0.0.1:${srv.address().port}/app/index.html`);
const r = await p.evaluate(async () => {
  const w = new Worker('/app/store-worker.js');
  let id = 0;
  const call = (msg, transfer = []) => new Promise((resolve) => {
    const my = ++id;
    w.addEventListener('message', function h(e) { if (e.data.id === my) { w.removeEventListener('message', h); resolve(e.data); } });
    w.postMessage({ id: my, ...msg }, transfer);
  });
  const blocks = (n, size, tag) => Array.from({ length: n }, (_, i) => [`b${tag}${String(i).padStart(6, '0')}`, crypto.getRandomValues(new Uint8Array(Math.min(size, 65536))).slice(0, size)]);
  const name = 'k51spike';
  await call({ op: 'drop', name });
  // A full board: 1 368 blocks, ~13.7 KB each on average.
  const full = blocks(1368, 13_700, 'f');
  let t = performance.now();
  await call({ op: 'apply', name, record: new Uint8Array(410), added: full, removed: [] }, full.map(([, x]) => x.buffer));
  const writeAll = performance.now() - t;
  // One publish: ~25 changed blocks (25–40 KiB in all, G.5.3) written, as many deleted.
  const upd = blocks(25, 1_500, 'u');
  t = performance.now();
  await call({ op: 'apply', name, record: new Uint8Array(410), added: upd, removed: full.slice(0, 25).map(([c]) => c) }, upd.map(([, x]) => x.buffer));
  const publish = performance.now() - t;
  // Startup: every file read back.
  t = performance.now();
  const loaded = await call({ op: 'load', name });
  const load = performance.now() - t;
  await call({ op: 'drop', name });
  // The main thread's createWritable, 200 files.
  const root = await navigator.storage.getDirectory();
  const d = await root.getDirectoryHandle('spike-main', { create: true });
  const few = blocks(200, 13_700, 'm');
  t = performance.now();
  for (const [c, x] of few) {
    const f = await (await d.getFileHandle(c, { create: true })).createWritable();
    await f.write(x);
    await f.close();
  }
  const mainEach = (performance.now() - t) / few.length;
  await root.removeEntry('spike-main', { recursive: true });
  return { writeAll, perFile: writeAll / full.length, publish, load, files: loaded.blocks.length, mainEach };
});
console.log(`full board, 1 368 files written by the Worker: ${r.writeAll.toFixed(0)} ms (${r.perFile.toFixed(2)} ms/file)`);
console.log(`a publish (25 written + 25 deleted): ${r.publish.toFixed(1)} ms`);
console.log(`startup, every file read back (${r.files}): ${r.load.toFixed(0)} ms`);
console.log(`main thread createWritable: ${r.mainEach.toFixed(2)} ms/file (Worker SyncAccessHandle: ${r.perFile.toFixed(2)})`);
await b.close();
srv.close();
