// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem boards (docs/BOARDS.md, BD-3): the page side. Rust (BoardApp, part of the Tor build)
// hosts, signs, serves and verifies; this file runs the Workers: the proof of work (1–4
// pow-worker.js on ephem_pow.wasm, compiled here once from its SHA-384-pinned bytes) and the
// hosted boards' block store (store-worker.js, OPFS). Hosted in the main app tab (R6). The
// screens come with BD-6; until then the offline lab drives this through `ephemBoards`.

const $meta = (n) => document.querySelector(`meta[name="${n}"]`)?.content;
let ctx = null;
let boards = null;                   // BoardApp
let powModule = null;                // Promise<WebAssembly.Module>
let store = null;                    // the store Worker
let storeSeq = 0;
const storeWaiting = new Map();      // request id → resolve
const names = new Map();             // hosted index → board name (the store's folder)

export function init(c) {
  ctx = c;
  if (!ctx.TOR || !ctx.ch) return;
  boards = new ctx.mod.BoardApp(ctx.ch);
  boards.set_listener((index) => persist(index));
  if (globalThis.ephemTorLab) globalThis.ephemBoards = { app: boards, host, reopen, post, resend, read, solve, stored };
}

// ---- the block store ----

function storeCall(msg, transfer = []) {
  if (!store) {
    store = new Worker(new URL('./store-worker.js', import.meta.url));
    store.onmessage = (e) => {
      const r = storeWaiting.get(e.data.id);
      storeWaiting.delete(e.data.id);
      r?.(e.data);
    };
  }
  const id = ++storeSeq;
  return new Promise((resolve, reject) => {
    storeWaiting.set(id, (d) => (d.error ? reject(new Error(d.error)) : resolve(d)));
    store.postMessage({ id, ...msg }, transfer);
  });
}

/** After a publish: the new blocks and record go to the store, unreachable blocks leave it. */
async function persist(index) {
  const name = names.get(index);
  if (!name) return;
  const d = boards.delta(index);
  // Transferred, not copied: the bytes were made for the store alone.
  const transfer = d.added.map(([, b]) => b.buffer).concat(d.record.length ? [d.record.buffer] : []);
  try {
    await storeCall({ op: 'apply', name, record: d.record, added: d.added, removed: d.removed }, transfer);
  } catch (e) {
    ctx.error?.(`The board could not be stored: ${e.message}`);
  }
}

/** Blocks and record the store holds for board `name` (lab check). */
export async function stored(name) {
  const r = await storeCall({ op: 'load', name });
  return { record: r.record?.length || 0, blocks: r.blocks.length };
}

// ---- owner ----

/** Creates board `index` (unless the store already holds it: then reopens it) and serves it. */
export async function host(index, title, about, rules) {
  const name = boards.name(index);
  const had = await storeCall({ op: 'load', name });
  if (had.record) boards.open(index, had.record, had.blocks);
  else boards.create(index, title, about, rules);
  names.set(index, name);
  await persist(index);
  return { name, onion: boards.serve(index) };
}

/** Reopens board `index` from the store (after a reload) and serves it. */
export async function reopen(index) {
  const name = boards.name(index);
  const had = await storeCall({ op: 'load', name });
  if (!had.record) throw new Error('this board is not stored here');
  boards.open(index, had.record, had.blocks);
  names.set(index, name);
  await persist(index);
  return { name, onion: boards.serve(index) };
}

// ---- poster ----

function module() {
  powModule ||= (async () => {
    const sri = $meta('ephem-pow-wasm');
    const res = await fetch(new URL('./pkg/ephem_pow.wasm', import.meta.url), sri ? { integrity: sri } : {});
    return WebAssembly.compile(await res.arrayBuffer());
  })();
  return powModule;
}

/** Solves `params` (Draft.params()) in up to 4 Workers; the first solution wins. */
export async function solve(params, onProgress) {
  const m = await module();
  const n = Math.min(4, Math.max(1, navigator.hardwareConcurrency || 2));
  const workers = [];
  let attempts = 0;
  try {
    return await new Promise((resolve, reject) => {
      for (let i = 0; i < n; i++) {
        const w = new Worker(new URL('./pow-worker.js', import.meta.url));
        workers.push(w);
        w.onerror = (e) => reject(new Error(e.message || 'proof-of-work worker failed'));
        w.onmessage = (e) => {
          if (e.data.solution) resolve(e.data);
          else onProgress?.((attempts += 4));
        };
        const start = crypto.getRandomValues(new Uint8Array(16));
        w.postMessage({ module: m, ...params, n: start });
      }
    });
  } finally {
    for (const w of workers) w.terminate();
  }
}

/** Opens a reply box, solves, signs and submits: resolves to `{no, seq, draft}`. */
export async function post(name, onion, thread, sub, body, sage, onProgress) {
  const draft = await boards.draft(name, onion, thread);
  const t0 = performance.now();
  const s = await solve(draft.params(), onProgress);
  const solveMs = performance.now() - t0;
  const r = JSON.parse(await boards.post_draft(draft, sub, body, sage, s.n, s.solution));
  return { ...r, draft, solveMs, effort: draft.effort_now };
}

/** The same submit again (a dropped answer): the host returns the original number. */
export async function resend(draft) {
  return JSON.parse(await boards.resend(draft));
}

// ---- reader ----

/** Reads and verifies board `name` with the given threads (numbers). */
export async function read(name, onions, threads = [], minSeq = 0) {
  return JSON.parse(await boards.read(name, onions, minSeq, threads.map(Number)));
}
