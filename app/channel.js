// Ephem public channels (docs/P2P-CHAT.md §27, Appendix D): page glue for channel.html.
// Rust (pkg/ephem_channel_bg.wasm) builds, signs, serves and verifies everything; this file
// stores the owner's channel (OPFS, IndexedDB where OPFS cannot write), renders, and forwards
// input. Nothing of the chat app is loaded here.
import * as slots from './slots.js';

// The same built-in Snowflake settings as Tor mode (app.js); the lab test hook `ephemTorLab`
// replaces them (a page script cannot set it: the CSP allows only our files).
const SNOWFLAKE = {
  broker: 'https://snowflake-broker.torproject.net/,https://1098762253.rsc.cdn77.org/',
  fingerprint: '2B280B23E1107BB62ABFC40DDCC8824814F80A72,8838024498816A039FCBBAB14E6F40A0843051FA',
  ice: 'stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478,stun:stun.sonetel.com:3478,stun:stun.uls.co.za:3478,stun:stun.voipgate.com:3478,stun:stun.voys.nl:3478',
  nat: '',
  network: '',
};
const INDEX = 0; // one channel per identity in this version (§D.3 allows more)

const $ = (id) => document.getElementById(id);
const enc = new TextEncoder();
let app, torUp = null, name = '', onion = '';

function status(label, cls = '') {
  $('status').textContent = label;
  $('status').className = 'pill status ' + cls;
}

function error(msg) {
  $('error').textContent = String(msg?.message || msg);
  $('error').hidden = !msg;
}

function show(view) {
  for (const v of document.querySelectorAll('.view')) v.hidden = v.id !== view;
}

// ---- storage: <kind>/<name>/{channel.car, record.bin}; kind = channels (own) | mirrors --------
async function opfsDir(kind, n, create) {
  const root = await navigator.storage.getDirectory();
  const dir = await root.getDirectoryHandle(kind, { create });
  return dir.getDirectoryHandle(n, { create });
}

function idb(mode, fn) {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('ephem-channels', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => reject(req.error);
    req.onsuccess = () => {
      const t = req.result.transaction('files', mode);
      const r = fn(t.objectStore('files'));
      t.oncomplete = () => { req.result.close(); resolve(r.result); };
      t.onerror = () => { req.result.close(); reject(t.error); };
    };
  });
}

async function store(car, record, kind = 'channels', n = name) {
  try {
    const dir = await opfsDir(kind, n, true);
    for (const [file, bytes] of [['channel.car', car], ['record.bin', record]]) {
      const w = await (await dir.getFileHandle(file, { create: true })).createWritable();
      await w.write(bytes);
      await w.close();
    }
  } catch {
    // Browsers without writable OPFS files (older Safari): the same bytes in IndexedDB.
    await idb('readwrite', (s) => { s.put(car, `${kind}/${n}/channel.car`); return s.put(record, `${kind}/${n}/record.bin`); });
  }
}

async function load(kind = 'channels', n = name) {
  try {
    const dir = await opfsDir(kind, n, false);
    const read = async (f) => new Uint8Array(await (await (await dir.getFileHandle(f)).getFile()).arrayBuffer());
    return { car: await read('channel.car'), record: await read('record.bin') };
  } catch {
    const car = await idb('readonly', (s) => s.get(`${kind}/${n}/channel.car`)).catch(() => undefined);
    const record = await idb('readonly', (s) => s.get(`${kind}/${n}/record.bin`)).catch(() => undefined);
    return car && record ? { car, record } : null;
  }
}

function download(bytes, file) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  a.download = file;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

// ---- rendering --------------------------------------------------------------------------------
function renderPosts(ol, view, owner) {
  ol.replaceChildren();
  const bySeq = new Map(view.posts.map((p) => [p.seq, p]));
  for (const p of [...view.posts].reverse()) {
    const li = document.createElement('li');
    li.dataset.seq = p.seq;
    const meta = document.createElement('div');
    meta.className = 'meta note';
    meta.textContent = `#${p.seq} · ${new Date(p.ts * 1000).toLocaleString()}`;
    li.append(meta);
    if (p.reply && bySeq.has(p.reply)) {
      const q = document.createElement('div');
      q.className = 'quote note';
      q.textContent = `↪ #${p.reply}: ${bySeq.get(p.reply).deleted ? '(deleted)' : bySeq.get(p.reply).body.slice(0, 120)}`;
      li.append(q);
    }
    const body = document.createElement('div');
    body.className = 'body';
    body.textContent = p.deleted ? '(deleted by the owner)' : p.body;
    if (p.deleted) li.classList.add('deleted');
    li.append(body);
    if (owner && !p.deleted) {
      const del = document.createElement('button');
      del.className = 'ghost';
      del.textContent = 'Delete';
      del.onclick = () => {
        if (!confirm('Delete this post? Followers may still have older copies.')) return;
        change(() => app.delete(p.seq));
      };
      li.append(del);
    }
    ol.append(li);
  }
  if (!view.posts.length) ol.innerHTML = '<li class="note">No posts yet.</li>';
}

// ---- owner ------------------------------------------------------------------------------------
function linkFor() {
  return `${location.origin}${location.pathname}#c=${name}&o=${onion}`;
}

function renderOwn() {
  const v = JSON.parse(app.view());
  $('o-title').textContent = v.title;
  $('o-about').textContent = v.about;
  $('i-mirrors').value = v.mirrors.join(', ');
  $('o-link').value = onion ? linkFor() + (v.mirrors.length ? ',' + v.mirrors.join(',') : '') : '';
  renderPosts($('o-posts'), v, true);
}

// Every change: Rust rebuilds and re-signs; the page stores the new CAR and record at once.
async function change(fn) {
  try {
    fn();
    await store(app.car(), app.record());
    renderOwn();
    error('');
  } catch (e) {
    error(e);
  }
}

async function goOnline() {
  status('starting Tor');
  try {
    await torUp;
    onion = app.serve();
    $('o-serving').textContent = `Online through Tor while this tab is open (${onion.slice(0, 8)}….onion).`;
    status('online', 'ok');
    renderOwn();
  } catch (e) {
    status('Tor failed', 'bad');
    error(e);
  }
}

async function openOwned() {
  name = app.channel_name(INDEX);
  const saved = await load();
  if (saved) {
    app.open(INDEX, saved.car, saved.record);
    await store(app.car(), app.record()); // a record older than 7 days was re-signed
  }
  $('signin').hidden = true;
  $('ch-new').hidden = !!saved;
  $('ch-open').hidden = !saved;
  if (saved) {
    renderOwn();
    goOnline();
  }
}

async function renderSlots() {
  const ul = $('slots');
  ul.replaceChildren();
  for (const s of await slots.list()) {
    const li = document.createElement('li');
    const b = document.createElement('button');
    b.textContent = `${s.label || '(no label)'} · ${s.handle}`;
    b.onclick = () => { $('slots').dataset.pick = s.id; for (const x of ul.querySelectorAll('button')) x.classList.toggle('primary', x === b); };
    li.append(b);
    ul.append(li);
  }
}

async function signIn() {
  try {
    const pick = $('slots').dataset.pick;
    const blob = pick ? (await slots.list()).find((s) => s.id === pick)?.blob : new Uint8Array(await $('i-file').files[0]?.arrayBuffer() ?? new ArrayBuffer(0));
    if (!blob?.length) return error('Choose a remembered identity or a key file.');
    const pass = enc.encode($('i-pass').value);
    $('i-pass').value = '';
    app.sign_in(blob, pass);
    status(`signed in: ${app.label()}`);
    await openOwned();
    error('');
  } catch (e) {
    error(e === 'E_KEYFILE_INVALID' ? 'Wrong passphrase, or the key file is damaged.' : e);
  }
}

async function create() {
  if (!$('c-understood').checked) return error('Please confirm that you understand the warnings.');
  try {
    app.create(INDEX, $('i-title').value.trim(), $('i-about').value.trim());
    await navigator.storage.persist?.().catch(() => false);
    await store(app.car(), app.record());
    $('ch-new').hidden = true;
    $('ch-open').hidden = false;
    renderOwn();
    goOnline();
    error('');
  } catch (e) {
    error(e);
  }
}

async function importBackup() {
  const files = [...$('i-import').files];
  const car = files.find((f) => f.name.endsWith('.car'));
  const rec = files.find((f) => !f.name.endsWith('.car'));
  if (!car || !rec) return error('Choose both files of the backup: channel.car and the record.');
  try {
    app.open(INDEX, new Uint8Array(await car.arrayBuffer()), new Uint8Array(await rec.arrayBuffer()));
    await store(app.car(), app.record());
    $('ch-new').hidden = true;
    $('ch-open').hidden = false;
    renderOwn();
    goOnline();
    error('');
  } catch (e) {
    error(e);
  }
}

// ---- reader -----------------------------------------------------------------------------------
let reading = null;
let reading_name = '';
const hwKey = (n) => `ephem-hw-${n}`;

function highWater(n) {
  try { return Number(localStorage.getItem(hwKey(n)) || 0); } catch { return 0; }
}

async function read(n, onions) {
  status('reading through Tor');
  try {
    await torUp;
    reading = await app.read(n, onions.join(','), highWater(n));
    reading_name = n;
    if (mirroring) await serveMirror(n, reading); // a newer version reached us: mirror it
    try { localStorage.setItem(hwKey(n), String(reading.sequence)); } catch { /* per-viewer convenience only */ }
    const v = JSON.parse(reading.json);
    $('r-title').textContent = v.title;
    $('r-about').textContent = v.about;
    $('r-source').textContent = `Verified: signed by the channel key, version ${v.sequence}, updated ${new Date(v.updated * 1000).toLocaleString()}.`;
    renderPosts($('r-posts'), v, false);
    status('read', 'ok');
    error('');
  } catch (e) {
    status('unreachable', 'bad');
    error(`The channel is not reachable right now (its owner and mirrors may be offline): ${e?.message || e}`);
  }
}

// ---- mirrors (§D.7.1) ------------------------------------------------------------------------
const MIRROR_REFRESH_MS = 10 * 60 * 1000;
let mirroring = false;

// A random seed per mirrored channel, kept in this browser: the mirror's onion address stays the
// same across visits (so the owner can sign it into the mirror list).
function mirrorSeed(n) {
  const key = `ephem-mirror-seed-${n}`;
  try {
    const hex = localStorage.getItem(key);
    if (hex) return Uint8Array.from(hex.match(/../g), (h) => parseInt(h, 16));
  } catch { /* storage blocked: a new address each visit */ }
  const seed = crypto.getRandomValues(new Uint8Array(32));
  try { localStorage.setItem(key, Array.from(seed, (b) => b.toString(16).padStart(2, '0')).join('')); } catch { /* as above */ }
  return seed;
}

async function serveMirror(n, r) {
  await torUp;
  const at = app.mirror(r, mirrorSeed(n));
  await store(r.car(), r.record(), 'mirrors', n);
  mirroring = true;
  $('mirror-text').textContent = `Mirroring while this tab is open, at ${at}. Send this address to the owner if you want to be in the signed mirror list.`;
  $('mirror-note').hidden = false;
  $('b-mirror').hidden = true;
  return at;
}

async function mirror() {
  if (!reading) return;
  try {
    await serveMirror(reading_name, reading);
    error('');
  } catch (e) {
    error(e);
  }
}

// A mirror stored in this browser: served again as soon as Tor is up, then kept current.
async function resumeMirror(n, onions) {
  const saved = await load('mirrors', n);
  if (!saved) return;
  try {
    await serveMirror(n, app.verify(n, saved.record, saved.car, 0));
  } catch (e) {
    return error(`The stored mirror could not be served: ${e?.message || e}`);
  }
  setInterval(async () => {
    try {
      const fresh = await app.read(n, onions.join(','), highWater(n));
      await serveMirror(n, fresh);
    } catch { /* owner offline: keep serving what we have */ }
  }, MIRROR_REFRESH_MS);
}

// ---- boot -------------------------------------------------------------------------------------
async function main() {
  const mod = await import('./pkg/ephem_channel.js');
  const wasmSri = document.querySelector('meta[name="ephem-wasm"]')?.content;
  await mod.default({ module_or_path: fetch(new URL('./pkg/ephem_channel_bg.wasm', import.meta.url), wasmSri ? { integrity: wasmSri } : {}) });
  app = new mod.ChannelApp();
  // Lab test hook (checks/tor-lab): with the lab's settings injected, the test may drive the
  // app directly (e.g. 1 000 posts for C-P3). Never set otherwise.
  if (globalThis.ephemTorLab) globalThis.ephemChannel = app;
  const c = globalThis.ephemTorLab || SNOWFLAKE;
  const log = globalThis.ephemTorLab?.log || globalThis.ephemTorLog;
  if (log) app.tor_log(log);
  torUp = app.tor_start(c.broker, c.fingerprint, c.ice, c.nat, c.network, new Uint8Array());
  torUp.catch((e) => { status('Tor failed', 'bad'); error(e); });
  const tick = setInterval(() => {
    const s = app.tor_status();
    if (/^100%|ready/.test(s)) clearInterval(tick);
    else if (s && !$('status').classList.contains('ok')) status(`Tor ${s.split(':')[0]}`);
  }, 1000);
  const build = document.querySelector('meta[name="ephem-build"]')?.content;
  if (build) $('build').textContent = `Build ${build}.`;

  const params = new URLSearchParams(location.hash.slice(1));
  if (params.get('c')) {
    const n = params.get('c');
    const onions = (params.get('o') || '').split(',').filter(Boolean);
    show('v-read');
    $('b-refresh').onclick = () => read(n, onions);
    $('b-mirror').onclick = mirror;
    $('b-kubo').onclick = () => {
      $('kubo').hidden = !$('kubo').hidden;
      $('kubo-cmds').textContent = `ipfs dag import channel.car\nipfs name put --allow-offline ${n} record.bin`;
    };
    $('b-dl-car').onclick = () => reading && download(reading.car(), 'channel.car');
    $('b-dl-record').onclick = () => reading && download(reading.record(), 'record.bin');
    await resumeMirror(n, onions);
    return read(n, onions);
  }
  show('v-own');
  status('sign in');
  renderSlots();
  $('b-signin').onclick = signIn;
  $('b-create').onclick = create;
  $('i-import').onchange = importBackup;
  $('b-copy').onclick = () => navigator.clipboard?.writeText($('o-link').value).catch(() => {});
  $('b-export').onclick = () => { download(app.car(), 'channel.car'); download(app.record(), 'record.bin'); };
  $('b-mirrors').onclick = () => change(() => app.set_mirrors($('i-mirrors').value));
  $('f-post').onsubmit = (e) => {
    e.preventDefault();
    const text = $('t-post').value.trim();
    if (!text) return;
    change(() => { app.post(text, 0); $('t-post').value = ''; });
  };
}

main().catch((e) => {
  status('error', 'bad');
  error(`Channels could not start: ${e?.message || e}`);
});
