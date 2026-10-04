// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// The boards' block store (docs/BOARDS.md G.5.3): one OPFS file per block,
// `<kind>/<name>/b/<cid>`, plus `record.bin`, written with SyncAccessHandle from this Worker
// (the page's createWritable costs milliseconds per file). `kind`: `boards` (hosted here) or
// `bmirrors` (this tab's mirrors, so a mirror serves its copy again at once after a reload,
// owner offline or not). Blocks arrive transferred, not copied; each is written once.
// Messages, answered in order:
//   { op: 'apply', kind, name, record, added: [[cid, Uint8Array]…], removed: [cid…] } → { ok }
//   { op: 'load', kind, name } → { record, blocks: [[cid, Uint8Array]…] } (empty when none)
//   { op: 'drop', kind, name } → { ok } (the whole board)

const KINDS = ['boards', 'bmirrors'];
const kindOf = (k) => (KINDS.includes(k) ? k : 'boards');

async function dir(kind, name, create) {
  const root = await navigator.storage.getDirectory();
  const boards = await root.getDirectoryHandle(kindOf(kind), { create });
  const b = await boards.getDirectoryHandle(name, { create });
  return { b, blocks: await b.getDirectoryHandle('b', { create }) };
}

async function put(d, file, bytes) {
  const h = await (await d.getFileHandle(file, { create: true })).createSyncAccessHandle();
  try {
    h.truncate(0);
    h.write(bytes, { at: 0 });
    h.flush();
  } finally {
    h.close();
  }
}

async function get(d, file) {
  const h = await (await d.getFileHandle(file)).createSyncAccessHandle();
  try {
    const out = new Uint8Array(h.getSize());
    h.read(out, { at: 0 });
    return out;
  } finally {
    h.close();
  }
}

const ops = {
  async apply({ kind, name, record, added, removed }) {
    const { b, blocks } = await dir(kind, name, true);
    for (const [cid, bytes] of added) await put(blocks, cid, bytes);
    // The record last: a crash before it leaves the previous version whole (its blocks are
    // removed only after the new record is written).
    if (record?.length) await put(b, 'record.bin', record);
    for (const cid of removed) await blocks.removeEntry(cid).catch(() => {});
    return { ok: true };
  },
  async load({ kind, name }) {
    let d;
    try { d = await dir(kind, name, false); } catch { return { record: null, blocks: [] }; }
    const record = await get(d.b, 'record.bin').catch(() => null);
    const blocks = [];
    for await (const [cid] of d.blocks.entries()) blocks.push([cid, await get(d.blocks, cid)]);
    return { record, blocks };
  },
  async drop({ kind, name }) {
    const root = await navigator.storage.getDirectory();
    const boards = await root.getDirectoryHandle(kindOf(kind), { create: true });
    await boards.removeEntry(name, { recursive: true }).catch(() => {});
    return { ok: true };
  },
};

// One operation at a time, in arrival order.
let chain = Promise.resolve();
self.onmessage = (e) => {
  chain = chain.then(async () => {
    try {
      const r = await ops[e.data.op](e.data);
      const transfer = r.blocks ? r.blocks.map(([, b]) => b.buffer) : [];
      self.postMessage({ id: e.data.id, ...r }, transfer);
    } catch (err) {
      self.postMessage({ id: e.data.id, error: String(err?.message || err) });
    }
  });
};
