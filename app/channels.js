// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem public channels (docs/P2P-CHAT.md §27, Appendix D and F.3.3): the Following and My
// channels tabs. Rust (ChannelApp, part of the Tor build) builds, signs, serves and verifies
// everything; this file keeps the lists, stores channels (OPFS, IndexedDB where OPFS cannot
// write), renders and forwards input.
//
// Tor mode: the chats' Tor client and identity (`App.bind_channels`). Direct mode: the Tor build
// is loaded the first time a channel tab opens, with its own Tor client; owning a channel then
// needs the passphrase once (the direct build cannot hand its identity across).
import * as slots from './slots.js';
import * as bridges from './bridges.js';
import { avatar } from './ui.js';

// Readers without Tor: channels mirrored to public IPFS (§D.6.3).
const GATEWAY = 'https://trustless-gateway.link';
// IPNS publishing (§D.5.2), reached through a Tor exit.
const ROUTING_HOST = 'delegated-ipfs.dev';
const REFRESH_MS = 10 * 60 * 1000;   // followed channels and mirrors
const PARALLEL = 4;                  // channels read at once (Tor circuits are not free)
const MAX_OWNED = 16;                // channel indices looked for in the store
const TLV_FOLLOWS = 0x06;            // the follow list in the key file (decision D2)

const $ = (id) => document.getElementById(id);
const enc = new TextEncoder();
let ctx = null;                      // from app.js: { TOR, app, mod, showPane, setTab, … }
let ch = null;                       // ChannelApp
let engine = null;                   // Promise of ch (direct mode: loading the Tor build)
let torUp = null;                    // Promise: the channels' Tor client is ready
let torResolve = null;
let follows = [];                    // [{ n, o: [onions], s: seq, t: title, seen, fresh, m }]
let owned = [];                      // [{ i, n, t, onion }]
let current = null;                  // on screen: { read: name } | { own: index }
const readings = new Map();          // name → the latest verified Reading
const sessionHw = new Map();         // name → sequence seen this session (channels not followed)
const signedMirrors = new Map();     // name → the mirror onions its last verified manifest names
let refreshing = false;

export function init(c) {
  ctx = c;
  torUp = new Promise((r) => { torResolve = r; });
  wire();
  if (ctx.TOR) {
    ch = new ctx.mod.ChannelApp();
    ctx.app.bind_channels(ch);
    engine = Promise.resolve(ch);
    if (globalThis.ephemTorLab) globalThis.ephemChannel = ch; // lab test hook (C-P3)
  }
  if (globalThis.ephemTorLab) globalThis.ephemChannelsRefresh = refreshAll; // lab: refresh now
}

/** Direct mode: the Tor part's own WebAssembly memory, once loaded (for the performance readout). */
let torWasm = null;
export function memories() {
  return torWasm ? [['wasm (Tor part)', torWasm.memory.buffer.byteLength]] : [];
}

/** Tor mode: the chats' Tor client is up (ev::TOR 2). */
export function torReady() {
  torResolve?.();
  torResolve = null;
  startAll();
}

/** The chat identity changed (sign-in, sign-out, a new saved identity). */
let boundTo = null;                  // the chat identity last handed to the channels
export function onIdentity() {
  if (!ctx) return;
  // Called on every return to the Chats tab: act only when the identity really changed.
  const who = ctx.app.identity_label() ? ctx.app.lock_name() : '';
  if (who === boundTo) return;
  boundTo = who;
  if (ctx.TOR && ch) ctx.app.bind_channels(ch);
  loadFollows();
  renderFollows();
  resetVault();
  if (ch) scanOwned().then(() => {
    if (!torResolve) startAll();
    else if (signedIn()) syncState('Starting Tor to look for this identity\'s channels on your other devices…');
  });
}

/** A temporary identity's follow list moves into its new key file. True if the file changed. */
export function moveToKeyFile() {
  if (!ramFollows.length) return false;
  const ok = ctx.app.set_section(TLV_FOLLOWS, JSON.stringify(ramFollows)) === 0;
  if (ok) ramFollows = [];
  return ok;
}

export async function openTab(tab) {
  ctx.setTab(tab);
  if (!(await ready())) return;
  if (tab === 'follow') {
    if (!torResolve) refreshAll();
    const f = current?.read && follows.find((x) => x.n === current.read);
    if (f) return showReader(f.n, f.o);
    return ctx.showPane(follows.length ? 'v-read' : 'v-follow-new');
  }
  const o = current?.own !== undefined && owned.find((x) => x.i === current.own);
  if (o) return showOwner(o.i);
  return owned.length ? showOwner(owned[0].i) : newChannel();
}

/** Phones: a channel tab shows its list; loads what it needs without opening a page. */
export async function prepare() {
  await ready();
}

/** A channel link (`#c=<name>&o=<onion>[,<mirror>…]`): the reader, in the Following tab. */
export async function openLink(text) {
  const frag = text.slice(text.indexOf('#') + 1);
  const p = new URLSearchParams(frag);
  const n = p.get('c');
  if (!n) return ctx.error('This is not a channel link.');
  ctx.setTab('follow');
  if (!(await ready())) return;
  showReader(n, (p.get('o') || '').split(',').filter(Boolean));
}

// ---- the engine (Tor build) and its Tor client --------------------------------------------------
async function ready() {
  if (!engine) engine = loadEngine();
  try {
    await engine;
    return true;
  } catch (e) {
    ctx.showPane('v-channels-off');
    $('ch-state').textContent = `Channels could not start: ${e?.message || e}`;
    return false;
  }
}

// Direct mode: the Tor build, fetched with the SHA-384 pinned in the page (§17.2).
async function loadEngine() {
  // The progress page, unless a phone is showing the tab's list (its page opens on a tap).
  if (!ctx.phone()) ctx.showPane('v-channels-off');
  $('ch-state').textContent = 'Loading the Tor part of Ephem…';
  const mod = await import('./pkg/ephem_tor.js');
  const sri = document.querySelector('meta[name="ephem-tor-wasm"]')?.content;
  torWasm = await mod.default({ module_or_path: fetch(new URL('./pkg/ephem_tor_bg.wasm', import.meta.url), sri ? { integrity: sri } : {}) });
  ch = new mod.ChannelApp();
  if (globalThis.ephemTorLab) globalThis.ephemChannel = ch;
  const lab = globalThis.ephemTorLab;
  const log = lab?.log || globalThis.ephemTorLog;
  if (log) ch.tor_log(log);
  // The user's bridges when signed in (Appendix F.2), else the Tor Project's Snowflake.
  const saved = ctx.app.identity_label() ? ctx.app.section(0x05) : '';
  const lines = lab?.bridges || (saved ? bridges.effective(saved) : bridges.DEFAULT_BRIDGES);
  ch.tor_start(lines, lab?.nat || '', lab?.network || '', new Uint8Array()).then(() => {
    $('ch-state').textContent = 'Tor ready.';
    torReady();
  }, (e) => {
    $('ch-state').textContent = `Tor failed: ${e?.message || e}`;
    ctx.setStatus('Tor failed', 'bad');
  });
  $('ch-state').textContent = 'Starting Tor…';
  await scanOwned();
  return ch;
}

// Once Tor is up: owned channels online, stored mirrors served, followed channels refreshed.
let running = false;
async function startAll() {
  if (!ch || torResolve) return;
  for (const o of owned) if (!o.onion && !o.away && !o.restoring) serveOwned(o);
  syncVault();
  if (running) return;
  running = true;
  // Reachability of owned channels changes on its own (publication, network changes).
  setInterval(() => {
    if (!owned.length) return;
    renderOwned();
    const o = owned.find((x) => x.i === current?.own);
    if (o && !$('v-own').hidden) renderServing(o);
  }, 3000);
  for (const f of follows) if (f.m) resumeMirror(f);
  refreshAll();
  setInterval(refreshAll, REFRESH_MS);
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

async function store(kind, n, car, record) {
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

async function load(kind, n) {
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

// ---- the follow list (decision D2: in the encrypted key file; RAM for a temporary identity) ----
let ramFollows = [];

function loadFollows() {
  if (ctx.app.identity_label()) {
    try { follows = JSON.parse(ctx.app.section(TLV_FOLLOWS) || '[]'); } catch { follows = []; }
  } else follows = ramFollows;
}

async function saveFollows() {
  // Only what identifies a channel and what was seen; posts are re-read.
  const list = follows.map(({ n, o, s, t, seen, m }) => ({ n, o, s, t, seen, m }));
  if (ctx.app.identity_label()) {
    if (ctx.app.set_section(TLV_FOLLOWS, JSON.stringify(list)) === 0) await ctx.persist();
  } else ramFollows = follows;
  renderFollows();
}

const highWater = (n) => follows.find((f) => f.n === n)?.s || sessionHw.get(n) || 0;

function renderFollows() {
  const ul = $('follows');
  ul.replaceChildren();
  let fresh = 0;
  for (const f of follows) {
    fresh += f.fresh || 0;
    const li = document.createElement('li');
    li.className = current?.read === f.n ? 'active' : '';
    li.innerHTML = '<span class="dot"></span><span class="grow"><b></b><span class="sub"></span></span>';
    li.querySelector('b').textContent = f.t || `${f.n.slice(0, 12)}…`;
    avatar(li, f.t || f.n);
    li.querySelector('.sub').textContent = f.err ? 'unreachable right now' : f.m ? 'mirroring' : f.last || '';
    if (f.fresh) {
      const b = document.createElement('span');
      b.className = 'badge';
      b.textContent = String(f.fresh);
      li.append(b);
    }
    li.onclick = () => { ctx.setTab('follow'); showReader(f.n, f.o); };
    ul.append(li);
  }
  $('follows-empty').hidden = follows.length > 0;
  $('badge-follow').hidden = !fresh;
  $('badge-follow').textContent = String(fresh);
}

// Every followed channel, a few at a time; counts the posts newer than the last one seen.
async function refreshAll() {
  if (refreshing || !follows.length) return;
  refreshing = true;
  const queue = [...follows];
  const worker = async () => {
    for (let f = queue.shift(); f; f = queue.shift()) {
      try {
        const r = await ch.read(f.n, f.o.join(','), f.s || 0);
        take(f, r);
        f.err = false;
        if (f.m) await serveMirror(f, r);
      } catch {
        f.err = true;
      }
      renderFollows();
    }
  };
  await Promise.all(Array.from({ length: PARALLEL }, worker));
  refreshing = false;
  saveFollows();
}

/** A verified reading of followed channel `f`: its title, sequence and new posts. */
function take(f, r) {
  readings.set(f.n, r);
  const v = JSON.parse(r.json);
  // Mirrors the owner signed into the channel become addresses to read from (§D.7.1): the
  // channel stays readable while its owner is offline, even from an old link.
  f.o = [...new Set([...f.o, ...v.mirrors])];
  const before = f.s || 0;
  f.s = r.sequence;
  f.t = v.title;
  const live = v.posts.filter((p) => !p.deleted);
  const newest = live.length ? Math.max(...live.map((p) => p.seq)) : 0;
  f.last = live.length ? live[live.length - 1].body.slice(0, 60) : 'no posts yet';
  const onScreen = current?.read === f.n && !$('v-read').hidden;
  if (onScreen) f.seen = newest;
  f.fresh = onScreen ? 0 : live.filter((p) => p.seq > (f.seen || 0)).length;
  if (f.fresh && r.sequence > before) {
    if (document.hidden) ctx.notify(`New post in ${f.t}`);
    const fresh = f.fresh;
    ctx.notice(`channel:${f.n}`, f.t, fresh === 1 ? 'New post' : `${fresh} new posts`, () => { ctx.setTab('follow'); showReader(f.n, f.o); });
  }
}

// ---- reader -----------------------------------------------------------------------------------
async function showReader(n, onions) {
  current = { read: n };
  ctx.showPane('v-read');
  const f = follows.find((x) => x.n === n);
  $('b-follow').hidden = !!f;
  $('b-unfollow').hidden = !f;
  $('b-mirror').hidden = !!f?.m;
  $('mirror-note').hidden = !f?.m;
  $('gateway-warn').hidden = true;
  $('kubo').hidden = true;
  $('b-refresh').onclick = () => read(n, onions);
  $('b-gateway-go').onclick = () => readViaGateway(n);
  $('b-kubo').onclick = () => {
    $('kubo').hidden = !$('kubo').hidden;
    $('kubo-cmds').textContent = `ipfs dag import channel.car\nipfs name put --allow-offline ${n} record.bin`;
  };
  $('b-follow').onclick = () => follow(n, onions);
  $('b-unfollow').onclick = () => unfollow(n);
  $('b-mirror').onclick = () => startMirror(n, onions);
  const cached = readings.get(n);
  if (cached) showReading(n, cached, 'through Tor');
  else {
    $('r-title').textContent = f?.t || 'Channel';
    $('r-about').textContent = '';
    $('r-source').textContent = 'Reading through Tor…';
    $('r-posts').replaceChildren();
  }
  renderFollows();
  return read(n, onions);
}

async function read(n, onions) {
  ctx.setStatus('reading through Tor');
  if (!onions.length) {
    ctx.error('This channel link names no onion address to read from.');
    $('gateway-warn').hidden = false;
    return;
  }
  try {
    await torUp;
    // The link's addresses, those the follow list learnt, and the signed mirrors seen last.
    const known = [...new Set([...onions, ...(follows.find((x) => x.n === n)?.o || []), ...(signedMirrors.get(n) || [])])];
    const r = await ch.read(n, known.join(','), highWater(n));
    signedMirrors.set(n, JSON.parse(r.json).mirrors);
    const f = follows.find((x) => x.n === n);
    if (f) {
      take(f, r);
      if (f.m) await serveMirror(f, r); // a newer version reached us: mirror it
      saveFollows();
    }
    readings.set(n, r);
    if (current?.read === n) showReading(n, r, 'through Tor');
  } catch (e) {
    ctx.setStatus('unreachable', 'bad');
    if (current?.read === n) {
      ctx.error(`The channel is not reachable right now (its owner and mirrors may be offline): ${e?.message || e}`);
      $('gateway-warn').hidden = false;
    }
  }
}

function showReading(n, r, how) {
  sessionHw.set(n, Math.max(sessionHw.get(n) || 0, r.sequence));
  const v = JSON.parse(r.json);
  $('r-title').textContent = v.title;
  $('r-about').textContent = v.about;
  $('r-source').textContent = `Verified ${how}: signed by the channel key, version ${v.sequence}, updated ${new Date(v.updated * 1000).toLocaleString()}.`;
  renderPosts($('r-posts'), v, null);
  const f = follows.find((x) => x.n === n);
  if (f) {
    const live = v.posts.filter((p) => !p.deleted);
    f.seen = live.length ? Math.max(...live.map((p) => p.seq)) : 0;
    f.fresh = 0;
    renderFollows();
  }
  ctx.setStatus('read', 'ok');
  $('error').hidden = true;
}

// A public IPFS gateway (§D.6.2): the reader's IP is visible to it (never the owner's); the
// record and every block are still verified here.
async function readViaGateway(n) {
  ctx.setStatus('reading through the gateway');
  try {
    const gw = globalThis.ephemTorLab?.gateway || GATEWAY;
    const get = async (path, accept) => {
      const r = await fetch(gw + path, { headers: { Accept: accept }, signal: AbortSignal.timeout(20_000) });
      if (!r.ok) throw new Error(`gateway: ${r.status}`);
      return new Uint8Array(await r.arrayBuffer());
    };
    const rec = await get(`/ipns/${n}?format=ipns-record`, 'application/vnd.ipfs.ipns-record');
    const root = ch.record_root(n, rec);
    const car = await get(`/ipfs/${root}?format=car&dag-scope=all`, 'application/vnd.ipld.car');
    const r = ch.verify(n, rec, car, highWater(n));
    readings.set(n, r);
    showReading(n, r, 'through the public gateway');
    $('gateway-warn').hidden = true;
  } catch (e) {
    ctx.setStatus('unreachable', 'bad');
    ctx.error(`Not available through the gateway (is it mirrored to IPFS?): ${e?.message || e}`);
  }
}

async function follow(n, onions) {
  if (follows.some((f) => f.n === n)) return;
  const r = readings.get(n);
  const v = r && JSON.parse(r.json);
  const live = v ? v.posts.filter((p) => !p.deleted) : [];
  follows.push({ n, o: onions, s: r?.sequence || 0, t: v?.title || '', seen: live.length ? Math.max(...live.map((p) => p.seq)) : 0, m: false, fresh: 0 });
  await saveFollows();
  $('b-follow').hidden = true;
  $('b-unfollow').hidden = false;
}

async function unfollow(n) {
  if (!confirm('Stop following this channel? A mirror of it stops too (at the next reload).')) return;
  follows = follows.filter((f) => f.n !== n);
  await saveFollows();
  $('b-follow').hidden = false;
  $('b-unfollow').hidden = true;
}

// ---- mirrors (§D.7.1) ------------------------------------------------------------------------
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

async function serveMirror(f, r) {
  await torUp;
  const at = ch.mirror(r, mirrorSeed(f.n));
  await store('mirrors', f.n, r.car(), r.record());
  f.at = at;
  if (current?.read === f.n) {
    $('mirror-text').textContent = `Mirroring while this tab is open, at ${at} (also readable in Tor Browser at http://${at}/). Send this address to the owner if you want to be in the signed mirror list.`;
    $('mirror-note').hidden = false;
    $('b-mirror').hidden = true;
  }
  return at;
}

async function startMirror(n, onions) {
  const r = readings.get(n);
  if (!r) return ctx.error('Read the channel first.');
  let f = follows.find((x) => x.n === n);
  if (!f) {
    await follow(n, onions);
    f = follows.find((x) => x.n === n);
  }
  try {
    f.m = true;
    await serveMirror(f, r);
    await saveFollows();
  } catch (e) {
    ctx.error(e?.message || e);
  }
}

// A mirror stored in this browser: served again as soon as Tor is up (kept current by refreshAll).
async function resumeMirror(f) {
  const saved = await load('mirrors', f.n);
  if (!saved) return;
  try {
    const r = ch.verify(f.n, saved.record, saved.car, 0);
    readings.set(f.n, r);
    await serveMirror(f, r);
  } catch (e) {
    ctx.error(`The stored mirror could not be served: ${e?.message || e}`);
  }
}

// ---- owner ------------------------------------------------------------------------------------
const linkBase = () => new URL('tor.html', location.href).href;
const linkFor = (o, v) => `${linkBase()}#c=${o.n}&o=${[o.onion, ...(v?.mirrors || [])].filter(Boolean).join(',')}`;

// The channels this identity owns: every index whose channel is in the store (no list to keep).
async function scanOwned() {
  const found = [];
  let label = '';
  try { label = ch.label(); } catch { /* not signed in */ }
  if (!label) {
    owned = found;
    return renderOwned();
  }
  for (let i = 0; i < MAX_OWNED; i++) {
    let n;
    try { n = ch.channel_name(i); } catch { break; }
    const saved = await load('channels', n);
    if (!saved) continue;
    try {
      ch.open(i, saved.car, saved.record);
      await store('channels', n, ch.car(i), ch.record(i)); // a record older than 7 days was re-signed
      // A channel already online keeps its address (serving is idempotent in Rust).
      found.push({ i, n, t: JSON.parse(ch.view(i)).title, onion: owned.find((o) => o.n === n)?.onion || '', away: !writer });
    } catch (e) {
      ctx.error(`Your channel ${i} could not be opened: ${e?.message || e}`);
    }
  }
  owned = found;
  renderOwned();
}

// Online means reachable: arti has published the onion's descriptor (usually under a minute
// after launch); before that, readers' dials fail.
const reachOf = (o) => (o.onion ? ch.reach(o.i) : '');
const REACH = { reachable: 'online through Tor', degraded: 'online through Tor', publishing: 'online, still publishing its address', unreachable: 'online, still publishing its address' };

function renderOwned() {
  const ul = $('owns');
  ul.replaceChildren();
  for (const o of owned) {
    const li = document.createElement('li');
    const r = reachOf(o);
    li.className = (current?.own === o.i ? 'active ' : '') + (o.onion ? 'ok' : '');
    li.innerHTML = '<span class="dot"></span><span class="grow"><b></b><span class="sub"></span></span>';
    li.querySelector('b').textContent = o.t || `Channel ${o.i}`;
    avatar(li, o.t || o.n);
    li.querySelector('.sub').textContent = o.restoring ? 'restoring from your other device…' : o.away ? 'written by your other device' : REACH[r] || 'offline';
    li.onclick = () => { ctx.setTab('own'); showOwner(o.i); };
    ul.append(li);
  }
  $('owns-empty').hidden = owned.length > 0;
  // Not signed in for channels: "No channel yet." would read as "this identity has none".
  $('owns-empty').textContent = signedIn() ? 'No channel yet.'
    : ctx.app.identity_label() ? 'Your identity\'s channels show here after you enter its passphrase once (direct mode: the channels part signs in on its own).'
      : 'Sign in with a saved identity to see its channels.';
  renderDiag();
}

async function serveOwned(o) {
  try {
    await torUp;
    o.onion = ch.serve(o.i);
    renderOwned();
    if (current?.own === o.i) renderOwn(o);
  } catch (e) {
    ctx.error(e?.message || e);
  }
}

function showOwner(i) {
  const o = owned.find((x) => x.i === i);
  if (!o) return newChannel();
  current = { own: i };
  if (o.away || o.restoring) return showAway(o);
  ctx.showPane('v-own');
  renderOwn(o);
  renderOwned();
}

function renderOwn(o) {
  const v = JSON.parse(ch.view(o.i));
  o.t = v.title;
  $('o-title').textContent = v.title;
  $('o-about').textContent = v.about;
  renderServing(o);
  $('i-mirrors').value = v.mirrors.join(', ');
  $('o-link').value = o.onion ? linkFor(o, v) : '';
  $('o-plain').hidden = !o.onion;
  $('o-plain-url').textContent = o.onion ? `http://${o.onion}/` : '';
  $('publish-state').textContent = '';
  $('o-missing').hidden = !v.missing;
  $('o-missing').textContent = v.missing ? `${v.missing} older post${v.missing === 1 ? ' is' : 's are'} not on this device yet: they join when your other device, a mirror or a backup has them. New posts continue the same channel.` : '';
  renderPosts($('o-posts'), v, o);
}

function renderServing(o) {
  const r = reachOf(o);
  const at = o.onion ? `${o.onion.slice(0, 8)}….onion` : '';
  $('o-serving').textContent = !o.onion ? 'Not online yet: waiting for Tor.'
    : `Online through Tor while this tab is open (${at}).${r === 'reachable' || r === 'degraded' ? '' : ' Tor is still publishing its address: some readers may not reach it for a minute or two.'}`;
}

// Every change: Rust rebuilds and re-signs; the page stores the new CAR and record at once.
async function change(o, fn) {
  try {
    fn();
    await store('channels', o.n, ch.car(o.i), ch.record(o.i));
    renderOwn(o);
    renderOwned();
    $('error').hidden = true;
    publishSoon();
  } catch (e) {
    ctx.error(e?.message || e);
  }
}

/** Who owns new channels here, and whether a sign-in is needed first. */
function renderIdentityNote() {
  let label = '';
  try { label = ch.label(); } catch { /* none */ }
  const chatId = ctx.app.identity_label();
  const separate = !!$('signin').dataset.separate;
  $('signin').hidden = !!label && !separate;
  $('ch-new').hidden = !label;
  $('ch-identity').textContent = label ? `Owned by your identity “${label}”: the channel's keys are derived from it.` : '';
  $('b-ch-separate').hidden = !ctx.TOR || !label || separate;
  $('signin-why').textContent = !ctx.TOR
    ? 'In direct mode the Tor part of Ephem needs your passphrase once to own channels.'
    : chatId
      ? 'A separate identity for channels: then its key file links nothing to your chats.'
      : 'A channel\'s keys come from a saved identity: sign in with one (or save this one under Chats → Your identity).';
  renderSlots();
}

async function renderSlots() {
  const ul = $('ch-slots');
  ul.replaceChildren();
  for (const s of await slots.list()) {
    const li = document.createElement('li');
    const b = document.createElement('button');
    b.textContent = `${s.label || '(no label)'} · ${s.handle}`;
    b.onclick = () => { ul.dataset.pick = s.id; for (const x of ul.querySelectorAll('button')) x.classList.toggle('primary', x === b); };
    li.append(b);
    ul.append(li);
  }
}

function newChannel() {
  current = null;
  ctx.showPane('v-own-new');
  renderIdentityNote();
  renderOwned();
}

async function signIn() {
  try {
    const pick = $('ch-slots').dataset.pick;
    const blob = pick ? (await slots.list()).find((s) => s.id === pick)?.blob : new Uint8Array(await $('i-ch-file').files[0]?.arrayBuffer() ?? new ArrayBuffer(0));
    if (!blob?.length) return ctx.error('Choose a remembered identity or a key file.');
    const pass = enc.encode($('i-ch-pass').value);
    $('i-ch-pass').value = '';
    ch.sign_in(blob, pass);
    delete $('signin').dataset.separate;
    resetVault();
    await scanOwned();
    for (const o of owned) serveOwned(o);
    syncVault();
    if (owned.length) showOwner(owned[0].i);
    else newChannel();
  } catch (e) {
    ctx.error(e === 'E_KEYFILE_INVALID' ? 'Wrong passphrase, or the key file is damaged.' : e?.message || e);
  }
}

const nextIndex = () => { for (let i = 0; i < MAX_OWNED; i++) if (!owned.some((o) => o.i === i)) return i; return -1; };

async function create() {
  if (!$('c-understood').checked) return ctx.error('Please confirm that you understand the warnings.');
  const i = nextIndex();
  if (i < 0) return ctx.error(`At most ${MAX_OWNED} channels per identity.`);
  try {
    ch.create(i, $('i-title').value.trim(), $('i-about').value.trim());
    const o = { i, n: ch.channel_name(i), t: '', onion: '' };
    await navigator.storage.persist?.().catch(() => false);
    await store('channels', o.n, ch.car(i), ch.record(i));
    owned.push(o);
    publishSoon();
    $('i-title').value = $('i-about').value = '';
    $('c-understood').checked = false;
    showOwner(i);
    serveOwned(o);
  } catch (e) {
    ctx.error(e?.message || e);
  }
}

async function importBackup() {
  const files = [...$('i-import').files];
  const car = files.find((f) => f.name.endsWith('.car'));
  const rec = files.find((f) => !f.name.endsWith('.car'));
  if (!car || !rec) return ctx.error('Choose both files of the backup: channel.car and the record.');
  const [c, r] = [new Uint8Array(await car.arrayBuffer()), new Uint8Array(await rec.arrayBuffer())];
  // The backup is one of this identity's channels: find which.
  for (let i = 0; i < MAX_OWNED; i++) {
    try {
      ch.open(i, c, r);
    } catch {
      continue;
    }
    const o = { i, n: ch.channel_name(i), t: '', onion: '' };
    await store('channels', o.n, ch.car(i), ch.record(i));
    owned = owned.filter((x) => x.i !== i).concat(o);
    showOwner(i);
    serveOwned(o);
    return;
  }
  ctx.error('This backup is not a channel of the signed-in identity.');
}

// ---- one identity on several devices: the vault (§D.11) ---------------------------------------
// The identity's devices share an encrypted IPNS record (Rust: channel::vault) listing its
// channels and which device writes them (the lease). A device that finds a live lease of
// another device shows its channels as "written by your other device" and offers to take
// over; one with no lease (or an expired one) writes: channels missing here are read from their
// onion or mirrors, else continued without their older posts (which join later).
const LEASE_S = globalThis.ephemTorLab?.leaseS || 15 * 60;
const RENEW_MS = (LEASE_S * 1000) / 3;
const RESTORE_MS = globalThis.ephemTorLab?.restoreMs || 90_000;
let vault = null;                    // { seq, device, until, channels: [{ index, title, count, mirrors }] }
let writer = true;                   // this device writes the identity's channels
let syncing = false;
let renewTimer = 0;
let publishTimer = 0;
let claimedAt = 0;                   // when this device last took over (ms)

/** A lab stand-in routing host and its test CA, or delegated-ipfs.dev. */
function routing() {
  const lab = globalThis.ephemTorLab?.routing;
  return [lab?.host || ROUTING_HOST, lab ? Uint8Array.from(atob(lab.root), (c) => c.charCodeAt(0)) : new Uint8Array()];
}

/** This browser's device id (random, kept in localStorage; a new one only after it is cleared). */
function deviceId() {
  let d = '';
  try { d = localStorage.getItem('ephem-device') || ''; } catch { /* storage blocked */ }
  if (!/^[0-9a-f]{32}$/.test(d)) {
    d = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');
    try { localStorage.setItem('ephem-device', d); } catch { /* per session then */ }
  }
  return d;
}

const signedIn = () => { try { return !!ch?.label(); } catch { return false; } };
const leasedElsewhere = (v) => !!v && v.device !== deviceId() && v.until * 1000 > Date.now();
const timeOf = (s) => new Date(s * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

/** The line under My channels that says what the vault sync is doing. */
function syncState(text) {
  $('own-sync').hidden = !text;
  $('own-sync').textContent = text;
  renderDiag();
}

// "Sync details" under My channels: what this device read and published, to compare devices
// when a channel does not show up. Nothing secret: short ids and times only.
const diag = { fetched: '', read: 'not yet', published: 'not yet' };
const hhmm = (d = new Date()) => d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
function renderDiag() {
  const on = signedIn();
  $('own-diag').hidden = !on;
  if (!on) return;
  let name = '';
  try { name = ch.vault_name(); } catch { /* not signed in */ }
  const v = vault;
  $('own-diag-text').textContent = [
    `this device      ${deviceId().slice(0, 8)}${writer ? ' (writes the channels)' : ' (another device writes them)'}`,
    `app version      ${document.querySelector('meta[name="ephem-build"]')?.content || '?'}`,
    `channel list     ${name ? `${name.slice(0, 10)}…${name.slice(-6)}` : '-'}`,
    `last read        ${diag.fetched ? `${diag.fetched}: ` : ''}${diag.read}`,
    `last published   ${diag.published}`,
    `list says        ${v ? `version ${v.seq}, ${v.channels.length} channel(s): ${v.channels.map((c) => c.title).join(', ') || '-'}; written by ${v.device.slice(0, 8)} until ${timeOf(v.until)}` : '-'}`,
    `on this device   ${owned.map((o) => `${o.t || o.i}${o.restoring ? ' (restoring)' : o.away ? ' (other device)' : o.onion ? ' (online)' : ''}`).join(', ') || '-'}`,
  ].join('\n');
}

/** Another identity: nothing known about its vault yet; this device writes until told. */
function resetVault() {
  vault = null;
  writer = true;
}

/** Fetches the vault and acts on its lease; `takeover`: this device writes from now on. */
async function syncVault(takeover = false) {
  if (!ch || !signedIn() || syncing) return;
  syncing = true;
  let found = null;                  // null: the vault could not be read
  try {
    if (torResolve) syncState('Starting Tor to look for this identity\'s channels on your other devices…');
    await torUp;
    syncState('Looking for this identity\'s channels (the list your devices share, through Tor)…');
    const [host, root] = routing();
    try {
      const j = await ch.vault_fetch(host, root);
      vault = j ? JSON.parse(j) : null;
      found = !!vault;
      diag.fetched = hhmm();
      diag.read = vault ? `found version ${vault.seq}` : 'no list published yet for this identity';
    } catch (e) {
      diag.fetched = hhmm();
      diag.read = `failed: ${e?.message || e}`;
      // Offline, or the routing service is down: keep going as we are (this device writes).
      console.info('vault:', e?.message || e);
      syncState(`Could not read the list of your channels (${e?.message || e}); trying again in a few minutes.`);
      if (!takeover) return;
    }
    if (leasedElsewhere(vault) && !takeover) {
      syncState('');
      return standDown();
    }
    const was = writer;
    writer = true;
    if (takeover) claimedAt = Date.now();
    if (!was || takeover) {
      await scanOwned(); // Rust released them: reopen from the store
      for (const o of owned) o.away = false;
    }
    // Listed at once, restored side by side (each can take up to RESTORE_MS).
    const todo = (vault?.channels || []).filter((e) => takeover || !owned.some((o) => o.i === e.index));
    for (const e of todo) if (!owned.some((o) => o.i === e.index)) owned.push({ i: e.index, n: ch.channel_name(e.index), t: e.title, onion: '', restoring: true });
    owned.sort((a, b) => a.i - b.i);
    renderOwned();
    if (todo.length) syncState(`Restoring ${todo.length} channel${todo.length === 1 ? '' : 's'} from your other device or mirrors…`);
    await Promise.all(todo.map((e) => restore(e, takeover)));
    // No list published yet (an older version, or the other device was never online since):
    // this identity's first channels may still be online at their own addresses.
    if (found === false && !owned.length) await probeOwned();
    syncState(found === false && !owned.length ? 'No channels found for this identity. If you created one on another device, open Ephem there (with Tor) once: it then publishes the list, and this device finds it.' : '');
    await publishVault();
    for (const o of owned) if (!o.onion) serveOwned(o);
    renderOwned();
    if (current?.own !== undefined && owned.some((o) => o.i === current.own)) showOwner(current.own);
    backfillOwned();
  } catch (e) {
    ctx.error(e?.message || e);
  } finally {
    syncing = false;
    clearTimeout(renewTimer);
    // Right after a takeover, look again soon: the other device may have renewed at the same
    // moment with the same sequence number, and the routing service keeps only one of them.
    renewTimer = setTimeout(renew, takeover ? Math.min(RENEW_MS, 15_000) : RENEW_MS);
  }
}

/** Another device holds the lease: stop hosting and posting here; list its channels. */
function standDown() {
  const lost = writer && owned.some((o) => o.onion);
  writer = false;
  ch.release();
  for (const o of owned) Object.assign(o, { onion: '', away: true });
  for (const e of vault.channels) {
    if (!owned.some((o) => o.i === e.index)) owned.push({ i: e.index, n: ch.channel_name(e.index), t: e.title, onion: '', away: true });
  }
  owned.sort((a, b) => a.i - b.i);
  if (lost) ctx.error(`Your other device took over your channels (until at least ${timeOf(vault.until)}). This tab stopped hosting them; take over again from My channels.`);
  renderOwned();
  if (current?.own !== undefined) showOwner(current.own);
}

/** Channel `e` of the vault on this device: the newest version from its onion or mirrors,
 *  else (none answers) continued without its older posts. `fresh`: re-read even if stored. */
async function restore(e, fresh) {
  const have = owned.find((o) => o.i === e.index && !o.restoring);
  if (have && !fresh) return;
  const n = ch.channel_name(e.index);
  const onions = [ch.channel_onion(e.index), ...e.mirrors].join(',');
  const local = have ? JSON.parse(ch.view(e.index) || '{"sequence":0}').sequence : 0;
  try {
    const r = await Promise.race([ch.read(n, onions, local), new Promise((_, no) => setTimeout(() => no(new Error('no host answered')), RESTORE_MS))]);
    if (!have || r.sequence > local) {
      ch.open(e.index, r.car(), r.record());
      await store('channels', n, ch.car(e.index), ch.record(e.index));
    }
  } catch (err) {
    if (have) return; // what the store has is what we write on
    console.info(`vault: channel ${e.index}: ${err?.message || err}; continuing without its older posts`);
    ch.resume(e.index);
    await store('channels', n, ch.car(e.index), ch.record(e.index));
  }
  const o = owned.find((x) => x.i === e.index);
  if (o) delete o.restoring;
  else owned.push({ i: e.index, n, t: e.title, onion: '' });
  owned.sort((a, b) => a.i - b.i);
  renderOwned();
}

/** Without a vault: the first channels of this identity read from their own onions (another
 *  device may be serving them). Found ones are kept here like restored ones. */
const PROBE = 4;
async function probeOwned() {
  syncState('No list of your channels yet: asking your first channel addresses directly…');
  await Promise.all(Array.from({ length: PROBE }, async (_, i) => {
    const n = ch.channel_name(i);
    try {
      const r = await Promise.race([ch.read(n, ch.channel_onion(i), 0), new Promise((_, no) => setTimeout(() => no(new Error('no answer')), RESTORE_MS))]);
      ch.open(i, r.car(), r.record());
      await store('channels', n, ch.car(i), ch.record(i));
      owned.push({ i, n, t: JSON.parse(ch.view(i)).title, onion: '' });
    } catch { /* no channel at this index, or its host is offline */ }
  }));
  owned.sort((a, b) => a.i - b.i);
  renderOwned();
}

/** Older posts missing here: joined from the channel's onion or mirrors when one has them. */
async function backfillOwned() {
  for (const o of owned) {
    const v = JSON.parse(ch.view(o.i) || '{}');
    if (!v.missing) continue;
    try {
      const onions = [...new Set([...(vault?.channels.find((e) => e.index === o.i)?.mirrors || []), ...v.mirrors])];
      if (!onions.length) continue;
      const r = await ch.read(o.n, onions.join(','), 0);
      ch.backfill(o.i, r.car());
      await store('channels', o.n, ch.car(o.i), ch.record(o.i));
      if (current?.own === o.i) renderOwn(o);
    } catch (e) {
      console.info(`vault: back-fill of channel ${o.i}: ${e?.message || e}`);
    }
  }
}

async function publishVault() {
  if (!writer || !signedIn()) return;
  try {
    await ch.vault_publish(...routing(), deviceId(), Math.floor(Date.now() / 1000) + LEASE_S);
    vault = JSON.parse(ch.vault());
    diag.published = `${hhmm()}: version ${vault.seq}, ${vault.channels.length} channel(s)`;
  } catch (e) {
    console.info('vault: not published:', e?.message || e);
    diag.published = `${hhmm()}: failed: ${e?.message || e}`;
  }
  renderDiag();
}

/** After a change: publish the vault once the burst of changes is over. */
function publishSoon() {
  clearTimeout(publishTimer);
  publishTimer = setTimeout(publishVault, 2000);
}

/** Every third of a lease: a writer checks nobody took over, then renews; a device that stood
 *  down takes over by itself when the other device's lease ran out. */
async function renew() {
  // While a fresh takeover may still be contested, look every 15 s.
  renewTimer = setTimeout(renew, Date.now() - claimedAt < LEASE_S * 1000 ? Math.min(RENEW_MS, 15_000) : RENEW_MS);
  if (!ch || !signedIn() || syncing) return;
  try {
    const j = await ch.vault_fetch(...routing());
    vault = j ? JSON.parse(j) : vault;
  } catch (e) {
    return console.info('vault:', e?.message || e);
  }
  if (leasedElsewhere(vault)) {
    // A device that took over within the last lease keeps its claim (a renewal of the other
    // device raced it); any other writer steps down.
    if (writer && Date.now() - claimedAt < LEASE_S * 1000) await publishVault();
    else if (writer) standDown();
  } else if (!writer) {
    syncVault();
  } else {
    await publishVault();
    backfillOwned();
  }
}

function showAway(o) {
  ctx.showPane('v-own-away');
  $('a-title').textContent = o.t || `Channel ${o.i}`;
  $('b-takeover').hidden = !!o.restoring;
  $('a-state').textContent = o.restoring
    ? 'Restoring this channel on this device: reading it from your other device or its mirrors through Tor (up to a minute and a half). If none of them is online, it continues here without its older posts, which join later.'
    : vault ? `Your other device writes this channel (its lease runs until at least ${timeOf(vault.until)}; it renews it while it runs).` : 'Your other device writes this channel.';
  renderOwned();
}

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
        change(owner, () => ch.delete(owner.i, p.seq));
      };
      li.append(del);
    }
    ol.append(li);
  }
  if (!view.posts.length) ol.innerHTML = '<li class="note">No posts yet.</li>';
}

// ---- controls ---------------------------------------------------------------------------------
function wire() {
  const own = () => owned.find((o) => o.i === current?.own);
  $('b-follow-new').onclick = async () => { ctx.setTab('follow'); if (await ready()) { current = null; ctx.showPane('v-follow-new'); renderFollows(); } };
  $('b-channel-open').onclick = () => openLink($('t-channel').value.trim());
  $('b-channel-scan').onclick = () => ctx.scan((t) => openLink(t));
  $('b-dl-car').onclick = () => { const r = readings.get(current?.read); if (r) ctx.download(r.car(), 'channel.car'); };
  $('b-dl-record').onclick = () => { const r = readings.get(current?.read); if (r) ctx.download(r.record(), 'record.bin'); };
  $('b-gateway').onclick = () => { $('gateway-warn').hidden = !$('gateway-warn').hidden; };
  $('b-own-new').onclick = async () => { ctx.setTab('own'); if (await ready()) newChannel(); };
  $('b-signin').onclick = signIn;
  $('b-ch-separate').onclick = () => { $('signin').dataset.separate = '1'; renderIdentityNote(); };
  $('b-create').onclick = create;
  $('i-import').onchange = importBackup;
  $('b-copy').onclick = () => navigator.clipboard?.writeText($('o-link').value).catch(() => {});
  $('b-export').onclick = () => { const o = own(); if (o) { ctx.download(ch.car(o.i), 'channel.car'); ctx.download(ch.record(o.i), 'record.bin'); } };
  $('b-takeover').onclick = () => syncVault(true);
  $('b-mirrors').onclick = () => { const o = own(); if (o) change(o, () => ch.set_mirrors(o.i, $('i-mirrors').value)); };
  $('b-publish').onclick = async () => {
    const o = own();
    if (!o) return;
    $('publish-state').textContent = 'publishing through Tor…';
    try {
      await torUp;
      await ch.publish_ipfs(o.i, ...routing());
      $('publish-state').textContent = `published (version ${JSON.parse(ch.view(o.i)).sequence})`;
    } catch (e) {
      $('publish-state').textContent = `failed: ${e?.message || e}`;
    }
  };
  $('f-post').onsubmit = (e) => {
    e.preventDefault();
    const o = own();
    const text = $('t-post').value.trim();
    if (!o || !text) return;
    change(o, () => { ch.post(o.i, text, 0); $('t-post').value = ''; });
  };
}
