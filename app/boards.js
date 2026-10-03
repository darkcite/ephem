// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem boards (docs/BOARDS.md): the page side. Rust (BoardApp, part of the Tor build) hosts,
// signs, serves and verifies; this file keeps the lists, renders, forwards input and runs the
// Workers: the proof of work (1–4 pow-worker.js on ephem_pow.wasm, compiled here once from its
// SHA-384-pinned bytes) and the hosted boards' block store (store-worker.js, OPFS).
//
// Boards live in the existing tabs, marked ▦ (R5): followed boards in Following (entries with
// `k: 'board'` in the channels' follow list), owned boards in My channels, hosted from this tab
// (R6). Links: `tor.html#B=<name>&o=<onion>[&m=<mirror>,…]`.
import * as channels from './channels.js';
import { avatar } from './ui.js';

const $ = (id) => document.getElementById(id);
const $meta = (n) => document.querySelector(`meta[name="${n}"]`)?.content;
const MAX_BOARDS = 4;
const UNDO_MS = 5_000;               // the host's undo window (crates/board host::UNDO_MS)
const REFRESH_MS = 30_000;           // an open board on screen (G.11.1)
let ctx = null;
let boards = null;                   // BoardApp
let starting = null;                 // Promise of boards (direct mode: loads the Tor part)
let powModule = null;                // Promise<WebAssembly.Module>
let store = null;                    // the store Worker
let storeSeq = 0;
const storeWaiting = new Map();      // request id → resolve
const names = new Map();             // hosted index → board name (the store's folder)
let owned = [];                      // [{ i, name, title, onion }]
let current = null;                  // on screen: { read, onions, thread } | { own, thread }
const views = new Map();             // board name → last verified view (shown at once, B-UX-3)
let box = null;                      // the reply box's pre-solve: { key, promise }
let refreshTimer = 0;
let ownTimer = 0;

export function init(c) {
  ctx = c;
  wire();
  channels.setBoardsHook(() => {
    renderFollows();
    scanOwned();
  }, () => {
    // The Following tab opened and only boards are followed: the one on screen last, or the first.
    if (current?.read) return showBoard(current.read, current.onions, current.thread), true;
    const f = follows()[0];
    return f ? (showBoard(f.n, f.o), true) : false;
  }, openLink);
  if (ctx.TOR && ctx.ch) adopt(new ctx.mod.BoardApp(ctx.ch));
  if (ctx.TOR) channels.torIsUp().then(resumeMirrors);
  setInterval(publishIpfs, 60_000);
}

function adopt(b) {
  boards = b;
  boards.set_listener((index, why) => {
    if (why === 'fenced') return standDown(index, 'Another of your devices published this board: it hosts it now, and this tab stopped.');
    persist(index);
    if (current?.own === index && !$('v-board-own').hidden) renderOwn();
  });
  if (globalThis.ephemTorLab) globalThis.ephemBoards = { app: boards, host, reopen, post, resend, read, solve, stored, canHost };
  return boards;
}

/** The BoardApp, loading the Tor part first in direct mode. */
function app() {
  if (boards) return Promise.resolve(boards);
  starting ||= channels.torEngine().then(({ ch, mod }) => adopt(new mod.BoardApp(ch)));
  return starting;
}

/** The BoardApp once its Tor client is up (reading, posting and mirroring need it). */
async function net() {
  const b = await app();
  await channels.torIsUp();
  return b;
}

const signedIn = () => !!ctx.app.identity_label();
const short = (n) => `${n.slice(0, 12)}…`;

// ---- links ----------------------------------------------------------------------------------

function linkFor(name, onion, mirrors = []) {
  const base = `${location.origin}${location.pathname.replace(/[^/]*$/, '')}tor.html`;
  return `${base}#B=${name}&o=${onion}${mirrors.length ? `&m=${mirrors.join(',')}` : ''}`;
}

/** A board link (`#B=<name>&o=<onion>[&m=<mirror>,…]`): the reader, in the Following tab. */
export async function openLink(text) {
  const p = new URLSearchParams(text.slice(text.indexOf('#') + 1));
  const name = p.get('B');
  if (!name) return ctx.error('This is not a board link.');
  const onions = [p.get('o'), ...(p.get('m') || '').split(',')].filter(Boolean);
  ctx.setTab('follow');
  showBoard(name, onions);
}

// ---- the block store ------------------------------------------------------------------------

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

/** Blocks and record the store holds for board `name`. */
export async function stored(name) {
  const r = await storeCall({ op: 'load', name });
  return { record: r.record?.length || 0, blocks: r.blocks.length };
}

// ---- owner ----------------------------------------------------------------------------------

/** Creates board `index` (unless the store already holds it: then reopens it) and serves it. */
export async function host(index, title, about, rules) {
  const b = await app();
  const name = b.name(index);
  const had = await storeCall({ op: 'load', name });
  if (had.record) b.open(index, had.record, had.blocks);
  else b.create(index, title, about, rules);
  names.set(index, name);
  await persist(index);
  return { name, onion: b.serve(index) };
}

/** Reopens board `index` from the store (after a reload) and serves it. */
export async function reopen(index) {
  const b = await app();
  const name = b.name(index);
  const had = await storeCall({ op: 'load', name });
  if (!had.record) throw new Error('this board is not stored here');
  b.open(index, had.record, had.blocks);
  names.set(index, name);
  await persist(index);
  return { name, onion: b.serve(index) };
}

let scanning = false;
/** The signed-in identity's boards in this browser's store (indices 0–3). */
async function scanOwned() {
  if (scanning || !ctx || (!boards && !ctx.TOR)) return renderOwned();
  scanning = true;
  try {
    const b = await app();
    const found = [];
    if (signedIn()) {
      const vault = channels.vaultApi.last();
      for (let i = 0; i < MAX_BOARDS; i++) {
        const name = b.name(i);
        const open = b.open_boards().includes(i);
        const entry = vault?.boards?.find((e) => e.index === i);
        if (open || entry || (await storeCall({ op: 'load', name })).record) {
          const prev = owned.find((o) => o.i === i && o.name === name);
          const o = prev || { i, name, title: entry?.title || '', onion: '' };
          o.entry = entry;
          o.away = !open && leasedElsewhere(entry);
          if (!open) o.onion = '';
          found.push(o);
        }
      }
    }
    owned = found;
  } catch {
    owned = [];
  } finally {
    scanning = false;
  }
  renderOwned();
  autoTakeOver();
}

// ---- several devices (G.13): one host per board, explicit takeovers ----------------------

/** Desktop Chrome and Firefox host boards; phones, Safari and the installed app do not (their
 *  background tabs are suspended, B-M5). */
export function canHost() {
  const ua = navigator.userAgent;
  const mobile = /Mobi|Android|iPhone|iPad|iPod/i.test(ua) || navigator.userAgentData?.mobile === true;
  const safari = /Safari\//.test(ua) && !/Chrome|Chromium|Firefox|Edg\//.test(ua);
  const pwa = matchMedia('(display-mode: standalone)').matches;
  return !mobile && !safari && !pwa;
}

const renewMs = () => (channels.vaultApi.leaseS() * 1000) / 3;
const leasedElsewhere = (e) => !!e && e.device !== channels.vaultApi.device() && e.until * 1000 > Date.now();
const claimedAt = new Map();         // index → when this device took the board over (ms)
let renewTimer = 0;

/** A host-capable device takes a board over by itself only once the other device's lease has
 *  been expired for two renewal periods (the routing service caches answers, A-m7); otherwise
 *  it offers "Host this board here". */
function autoTakeOver() {
  if (!canHost()) return;
  for (const o of owned) {
    const e = o.entry;
    if (o.away || o.onion || !e || e.device === channels.vaultApi.device() || boards.open_boards().includes(o.i)) continue;
    if (e.until * 1000 + 2 * renewMs() <= Date.now() && !o.taking) takeOver(o.i);
  }
}

/** The page's lower bound for numbers (G.13.6): the other device could not post past its lease. */
function numberFloor(e) {
  if (!e) return 1;
  return e.next_no + 120 * Math.max(0, Math.ceil((e.until - e.updated) / 60));
}

async function takeOver(i) {
  const o = owned.find((x) => x.i === i);
  if (!o || o.taking) return;
  if (!canHost()) return ctx.error('Hosting a board needs desktop Chrome or Firefox: phones, Safari and the installed app stop background tabs.');
  o.taking = true;
  $('ba-state').textContent = 'Reading the board from your other device or its mirrors through Tor (up to a few minutes)…';
  $('b-ba-host').disabled = true;
  try {
    const b = await net();
    await b.take_over(i, (o.entry?.mirrors || []).join(','), numberFloor(o.entry));
    o.name = b.name(i);
    names.set(i, o.name);
    await persist(i);
    o.onion = b.serve(i);
    o.away = false;
    claimedAt.set(i, Date.now());
    await channels.vaultApi.publishBoards(b).catch((e) => console.info('vault:', e?.message || e));
    scheduleRenew();
    renderOwned();
    showOwn(i);
  } catch (e) {
    $('ba-state').textContent = `The board could not be read from your other device or its mirrors (${e?.message || e}). You can continue it here without its posts: numbers continue, its threads come back if a mirror still has them.`;
    $('b-ba-continue').hidden = false;
  } finally {
    o.taking = false;
    $('b-ba-host').disabled = !canHost();
  }
}

async function continueHere(i) {
  const o = owned.find((x) => x.i === i);
  const e = o?.entry;
  try {
    const b = await net();
    b.continue_board(i, e?.title || o?.title || 'Board', numberFloor(e), e?.seq || 0, (e?.mirrors || []).join(','));
    names.set(i, b.name(i));
    await persist(i);
    o.onion = b.serve(i);
    o.away = false;
    claimedAt.set(i, Date.now());
    await channels.vaultApi.publishBoards(b).catch(() => {});
    scheduleRenew();
    showOwn(i);
  } catch (err) {
    ctx.error(`The board could not continue here: ${err?.message || err}`);
  }
}

/** Another device hosts board `i` now: stop here, say why. */
function standDown(i, why) {
  boards.close(i);
  const o = owned.find((x) => x.i === i);
  if (o) Object.assign(o, { onion: '', away: true, why });
  renderOwned();
  if (current?.own === i) showAway(o);
}

function showAway(o) {
  ctx.showPane('v-board-away');
  $('ba-title').textContent = `▦ ${o.title || short(o.name)}`;
  const e = o.entry;
  $('ba-state').textContent = o.why || (e && e.until * 1000 > Date.now()
    ? `Your other device hosts this board (its lease runs until at least ${new Date(e.until * 1000).toLocaleTimeString()}; it renews it while it runs).`
    : 'No device hosts this board right now.');
  $('b-ba-host').disabled = !canHost();
  $('ba-host-note').hidden = canHost();
  $('b-ba-continue').hidden = true;
}

/** Every third of a lease: a host checks nobody took its boards over, then renews its leases. */
function scheduleRenew() {
  clearTimeout(renewTimer);
  renewTimer = setTimeout(renew, renewMs());
}

async function renew() {
  scheduleRenew();
  if (!boards || !signedIn() || !boards.open_boards().length) return;
  let vault;
  try { vault = await channels.vaultApi.fetch(); } catch (e) { return console.info('vault:', e?.message || e); }
  for (const i of boards.open_boards()) {
    const e = vault?.boards?.find((x) => x.index === i);
    // A takeover within the last lease keeps its claim (a renewal of the other device raced it).
    if (leasedElsewhere(e) && Date.now() - (claimedAt.get(i) || 0) > channels.vaultApi.leaseS() * 1000) {
      standDown(i, 'Your other device hosts this board now: this tab stopped hosting it.');
    }
  }
  if (boards.open_boards().length) await channels.vaultApi.publishBoards(boards).catch((e) => console.info('vault:', e?.message || e));
}

function renderOwned() {
  const ul = $('board-owns');
  ul.replaceChildren();
  for (const o of owned) {
    const li = row(o.title || short(o.name), o.onion ? 'online' : o.away ? 'hosted on your other device' : 'stored here: tap to host', current?.own === o.i);
    li.className += o.onion ? ' ok' : '';
    li.onclick = () => { ctx.setTab('own'); showOwn(o.i); };
    ul.append(li);
  }
}

function row(title, sub, active) {
  const li = document.createElement('li');
  li.className = `board${active ? ' active' : ''}`;
  li.innerHTML = '<span class="dot"></span><span class="grow"><b></b><span class="sub"></span></span>';
  li.querySelector('b').textContent = `▦ ${title}`;
  avatar(li, title);
  li.querySelector('.sub').textContent = sub;
  return li;
}

function showNew() {
  ctx.setTab('own');
  ctx.showPane('v-board-new');
  if (!canHost()) {
    $('bn-identity').textContent = 'Hosting a board needs desktop Chrome or Firefox: phones, Safari and the installed app stop background tabs. You can read and post on boards here.';
    $('b-board-create').disabled = true;
    return;
  }
  $('bn-identity').textContent = signedIn()
    ? `Owned by your identity “${ctx.app.identity_label()}”: the board's keys are derived from it.`
    : 'Boards need a saved identity: save or sign in to one in Settings → Your identity first.';
  $('b-board-create').disabled = !signedIn();
}

async function create() {
  if (!$('bn-understood').checked) return ctx.error('Please read the warnings and tick “I understand”.');
  const title = $('bn-title').value.trim();
  if (!title) return ctx.error('A board needs a title.');
  await scanOwned();
  const used = new Set(owned.map((o) => o.i));
  const i = [...Array(MAX_BOARDS).keys()].find((x) => !used.has(x));
  if (i === undefined) return ctx.error('An identity can own at most 4 boards.');
  try {
    const r = await host(i, title, $('bn-about').value, $('bn-rules').value);
    owned.push({ i, name: r.name, title, onion: r.onion });
    renderOwned();
    showOwn(i);
  } catch (e) {
    ctx.error(`The board could not be created: ${e?.message || e}`);
  }
}

async function showOwn(i, thread = 0) {
  current = { own: i, thread };
  ctx.showPane('v-board-own');
  renderOwned();
  const o = owned.find((x) => x.i === i);
  if (o?.away || (o && !(await app()).open_boards().includes(i) && leasedElsewhere(o.entry))) return showAway(o);
  if (o && !o.onion && !canHost()) return showAway(Object.assign(o, { why: 'Hosting a board needs desktop Chrome or Firefox: phones, Safari and the installed app stop background tabs.' }));
  try {
    const b = await app();
    if (!b.open_boards().includes(i)) {
      $('bo-state').textContent = 'Opening the board from this browser\'s store…';
      const r = await reopen(i);
      if (o) o.onion = r.onion;
    } else if (o && !o.onion) o.onion = JSON.parse(b.status(i)).onion;
  } catch (e) {
    $('bo-state').textContent = `The board could not be opened: ${e?.message || e}`;
    return;
  }
  renderOwned();
  renderOwn();
  claimedAt.set(i, claimedAt.get(i) || Date.now());
  channels.vaultApi.publishBoards(boards).catch((e) => console.info('vault:', e?.message || e));
  scheduleRenew();
  // The onion's reachability and the counters change without a publish.
  clearInterval(ownTimer);
  ownTimer = setInterval(() => {
    if (current?.own === i && !$('v-board-own').hidden && !document.activeElement?.closest?.('#v-board-own')) renderOwn();
  }, 3_000);
}

/** The owner view of the board on screen (local: what this tab serves). */
function renderOwn() {
  const i = current?.own;
  if (i === undefined || !boards) return;
  const st = JSON.parse(boards.status(i) || '{}');
  let v;
  try { v = JSON.parse(boards.owner_view(i, current.thread ? [current.thread] : [])); } catch { return; }
  const o = owned.find((x) => x.i === i);
  if (o) o.title = v.title;
  $('bo-title').textContent = `▦ ${v.title}`;
  const reach = boards.reach(i);
  const online = { reachable: 'Online through Tor', degraded: 'Online through Tor (degraded)', publishing: 'Publishing its onion…', unreachable: 'Tor is still setting up its onion' }[reach] || 'Offline';
  $('bo-state').textContent = `${online} · ${st.threads} threads · next No. ${st.next_no} · version ${st.seq}`
    + (st.closed_notice ? ' · the board closed itself under a flood (switches below)' : '');
  $('bo-link').value = linkFor(v.name, st.onion, v.mirrors);
  $('bo-plain').textContent = `http://${st.onion}/`;
  const sw = JSON.parse(boards.switches(i));
  for (const k of Object.keys(sw)) $(`bs-${k}`).checked = sw[k];
  if (document.activeElement !== $('bo-eff-reply')) $('bo-eff-reply').value = st.base_reply;
  if (document.activeElement !== $('bo-eff-thread')) $('bo-eff-thread').value = st.base_thread;
  if (document.activeElement !== $('bo-mirrors')) $('bo-mirrors').value = v.mirrors.join(', ');
  if (document.activeElement !== $('bo-see')) $('bo-see').value = v.see_also.join(', ');
  $('bo-ipfs').checked = ipfsOn(v.name);
  // Held posts (pre-moderation).
  const held = JSON.parse(boards.held(i));
  $('bo-held-card').hidden = !held.length;
  $('bo-held').replaceChildren(...held.map((h) => {
    const li = postItem({ no: 0, ts: h.at, sub: h.sub, body: h.body, trip: h.trip, cap: 0, sage: false, del: 0 }, h.t ? `reply to No. ${h.t}` : 'new thread');
    li.append(modButtons([['Approve', () => act(() => boards.approve(i, h.i))], ['Reject', () => act(() => boards.reject(i, h.i))]]));
    return li;
  }));
  renderCatalog($('bo-catalog'), v, (no) => { current.thread = no; renderOwn(); });
  const t = v.threads.find((x) => x.no === current.thread);
  $('bo-thread').replaceChildren();
  if (t) {
    const ol = document.createElement('ol');
    ol.className = 'log posts';
    for (const p of t.posts) {
      const li = postItem(p);
      if (!p.del) li.append(modButtons(ownerActions(i, p, t.no)));
      ol.append(li);
    }
    $('bo-thread').append(ol);
  }
  $('bo-modlog').replaceChildren(...v.modlog.slice(-50).reverse().map((m) => {
    const li = document.createElement('li');
    li.textContent = `${new Date(m.ts * 1000).toLocaleString()} · ${m.act} No. ${m.no}${m.why ? ` (${m.why})` : ''}`;
    return li;
  }));
}

function ownerActions(i, p, thread) {
  const a = [['Delete', () => del(i, p.no)]];
  if (p.trip) {
    a.push(['Ban trip', () => act(() => boards.ban(i, p.no, ''))]);
    a.push(['Approve trip', () => act(() => boards.approve_trip_of(i, p.no, true))]);
    a.push(['Delete all of this trip', () => act(() => boards.delete_by_key_of(i, p.no))]);
  }
  if (p.no === thread) {
    const row = JSON.parse(boards.owner_view(i, [])).catalog.find((c) => c.no === thread);
    a.push([row?.lk ? 'Unlock' : 'Lock', () => act(() => boards.set_locked(i, thread, !row?.lk))]);
    a.push([row?.st ? 'Unstick' : 'Sticky', () => act(() => boards.set_sticky(i, thread, !row?.st))]);
    a.push(['Prune', () => act(() => boards.prune(i, thread))]);
  }
  return a;
}

function act(f) {
  try {
    f();
  } catch (e) {
    ctx.error(String(e?.message || e));
  }
  setTimeout(renderOwn, 1_500);
}

let undoTimer = 0;
function del(i, no) {
  act(() => boards.delete(i, no));
  $('bo-undo-text').textContent = `Deleting No. ${no}…`;
  $('bo-undo').hidden = false;
  $('b-bo-undo').onclick = () => {
    boards.undo(i, no);
    $('bo-undo').hidden = true;
  };
  clearTimeout(undoTimer);
  undoTimer = setTimeout(() => { $('bo-undo').hidden = true; renderOwn(); }, UNDO_MS + 1_500);
}

function setSwitches() {
  const i = current?.own;
  if (i === undefined) return;
  const v = (k) => $(`bs-${k}`).checked;
  boards.set_switches(i, v('paused'), v('threads_closed'), v('trips_only'), v('approved_only'), v('premod'), v('panic_trips'));
}

// ---- reader ---------------------------------------------------------------------------------

const follows = () => channels.followList().filter((f) => f.k === 'board');

function renderFollows() {
  const ul = $('board-follows');
  if (!ul) return;
  ul.replaceChildren();
  for (const f of follows()) {
    const li = row(f.t || short(f.n), f.err ? 'unreachable right now' : f.stale ? 'host offline (stale)' : `${f.up === false ? 'host offline · ' : ''}${f.last || ''}`, current?.read === f.n);
    li.onclick = () => { ctx.setTab('follow'); showBoard(f.n, f.o); };
    ul.append(li);
  }
}

async function showBoard(name, onions, thread = 0) {
  current = { read: name, onions, thread };
  ctx.showPane('v-board');
  renderFollows();
  const cached = views.get(name) || cachedView(name);
  if (cached) renderBoard(cached);
  else {
    $('bd-title').textContent = '▦ Board';
    $('bd-source').textContent = 'Reading through Tor…';
    $('bd-catalog').replaceChildren();
    $('bd-posts').replaceChildren();
  }
  setBox();
  await refresh();
  clearTimeout(refreshTimer);
  refreshTimer = setInterval(() => { if (!$('v-board').hidden && current?.read === name) refresh(); }, REFRESH_MS);
}

async function refresh() {
  const c = current;
  if (!c?.read) return;
  const f = follows().find((x) => x.n === c.read);
  const known = views.get(c.read);
  const onions = [...new Set([...c.onions, ...(f?.o || []), ...(known?.mirrors || [])])];
  try {
    const b = await net();
    const v = JSON.parse(await b.read(c.read, onions.join(','), f?.s || 0, c.thread ? [c.thread] : []));
    views.set(c.read, v);
    cacheView(v);
    if (f) {
      f.t = v.title;
      f.s = v.sequence;
      f.o = [...new Set([...f.o, ...v.mirrors])];
      f.err = false;
      f.stale = v.stale;
      f.last = v.catalog[0] ? (v.catalog[0].sub || v.catalog[0].ex).slice(0, 60) : 'no threads yet';
      channels.saveFollowList();
    }
    if (current === c) renderBoard(v);
  } catch (e) {
    if (f) f.err = true;
    if (current === c) $('bd-source').textContent = views.has(c.read) ? `Showing the last verified version: the board did not answer (${e?.message || e}).` : `The board could not be read: ${e?.message || e}`;
  }
  renderFollows();
}

function renderBoard(v) {
  const c = current;
  $('bd-title').textContent = `▦ ${v.title}`;
  $('bd-about').textContent = v.about;
  $('bd-rules').textContent = v.rules;
  $('bd-rules-box').hidden = !v.rules;
  $('bd-source').textContent = v.stale
    ? `Board not updated since ${new Date(v.updated * 1000).toLocaleString()}; the host is offline. Reading only.`
    : `Verified through Tor: signed by the board key, version ${v.sequence}, updated ${new Date(v.updated * 1000).toLocaleString()}.`;
  $('bd-source').classList.toggle('stale', !!v.stale);
  const followed = follows().some((f) => f.n === v.name);
  $('b-bd-follow').hidden = followed;
  $('b-bd-unfollow').hidden = !followed;
  $('b-bd-catalog').hidden = !c.thread;
  const t = c.thread && v.threads.find((x) => x.no === c.thread);
  $('bd-catalog').hidden = !!c.thread;
  $('bd-tools').hidden = !!c.thread;
  if (!c.thread) renderCatalog($('bd-catalog'), sorted(v), (no) => { current.thread = no; setBox(); refresh(); });
  $('bd-see').hidden = !v.see_also?.length;
  $('bd-see').replaceChildren(...(v.see_also?.length ? ['See also: ', ...v.see_also.flatMap((l) => {
    const [n, o] = l.split('@');
    const a = document.createElement('a');
    a.href = `#B=${n}&o=${o}`;
    a.textContent = `${n.slice(0, 14)}…`;
    a.onclick = (e) => { e.preventDefault(); showBoard(n, [o]); };
    return [a, ' '];
  })] : []));
  $('bd-posts').replaceChildren(...(t ? t.posts.map((p) => postItem(p)) : []));
  if (c.thread && !t) $('bd-source').textContent += ' This thread is no longer on the board (pruned or deleted).';
}

function renderCatalog(ol, v, open) {
  ol.replaceChildren(...v.catalog.map((t) => {
    const li = document.createElement('li');
    li.innerHTML = '<span class="grow"><b></b><span class="sub"></span></span>';
    li.querySelector('b').textContent = `No. ${t.no} ${t.sub}${t.st ? ' 📌' : ''}${t.lk ? ' 🔒' : ''}`;
    li.querySelector('.sub').textContent = `${t.r} replies · ${t.ex}`;
    li.className = current?.thread === t.no ? 'active' : '';
    li.onclick = () => open(t.no);
    return li;
  }));
}

/** One post: No., time, trip, capcode, sage, then the body with greentext (text nodes only). */
function postItem(p, extra = '') {
  const li = document.createElement('li');
  if (p.del) li.className = 'deleted';
  const meta = document.createElement('div');
  meta.className = 'meta note';
  meta.textContent = `${p.no ? `No. ${p.no}` : 'held'} · ${new Date(p.ts * 1000).toLocaleString()}${extra ? ` · ${extra}` : ''}`;
  if (p.trip) {
    const s = document.createElement('span');
    s.className = 'trip';
    s.textContent = ` · ${p.trip}`;
    meta.append(s);
  }
  if (p.cap === 1) {
    const s = document.createElement('span');
    s.className = 'cap';
    s.textContent = ' · ## Owner';
    meta.append(s);
  }
  if (p.sage) meta.append(' · sage');
  li.append(meta);
  if (p.sub) {
    const h = document.createElement('b');
    h.textContent = p.sub;
    li.append(h);
  }
  const body = document.createElement('div');
  body.className = 'body pre';
  if (p.del) body.textContent = p.del === 3 ? '(deleted by its poster)' : '(deleted)';
  else {
    p.body.split('\n').forEach((line, k) => {
      if (k) body.append('\n');
      if (line.startsWith('>') && !line.startsWith('>>')) {
        const g = document.createElement('span');
        g.className = 'gt';
        g.textContent = line;
        body.append(g);
      } else body.append(line);
    });
  }
  li.append(body);
  return li;
}

function modButtons(list) {
  const d = document.createElement('div');
  d.className = 'mod';
  for (const [label, f] of list) {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = label;
    b.onclick = (e) => { e.stopPropagation(); f(); };
    d.append(b);
  }
  return d;
}

async function follow() {
  const c = current;
  const v = views.get(c?.read);
  if (!v) return;
  const list = channels.followList();
  if (!list.some((f) => f.n === v.name)) list.push({ n: v.name, o: [...new Set([...c.onions, ...v.mirrors])], s: v.sequence, t: v.title, k: 'board' });
  await channels.saveFollowList();
  renderBoard(v);
}

async function unfollow() {
  const list = channels.followList();
  const i = list.findIndex((f) => f.n === current?.read && f.k === 'board');
  if (i >= 0) list.splice(i, 1);
  await channels.saveFollowList();
  const v = views.get(current?.read);
  if (v) renderBoard(v);
}

function mirrorSeed(n) {
  const k = `ephem-board-mirror:${n}`;
  let s = null;
  try { s = localStorage.getItem(k); } catch { /* storage blocked */ }
  if (!s) {
    s = [...crypto.getRandomValues(new Uint8Array(32))].map((x) => x.toString(16).padStart(2, '0')).join('');
    try { localStorage.setItem(k, s); } catch { /* the mirror's address changes next visit */ }
  }
  return Uint8Array.from(s.match(/../g).map((x) => parseInt(x, 16)));
}

async function mirror() {
  const c = current;
  if (!c?.read) return;
  const note = $('bd-mirror-note');
  note.hidden = false;
  note.textContent = 'Starting the mirror…';
  try {
    const b = await net();
    const onion = await b.mirror(c.read, c.onions.join(','), mirrorSeed(c.read));
    note.textContent = `Mirroring on ${onion}. It refreshes every 10 s while this tab is open, and starts again when Ephem opens. Send this address to the board's owner to sign it into the board.`;
    rememberMirror(c.read, c.onions);
    $('bd-mirror-ipfs-row').hidden = false;
    $('bd-mirror-ipfs').checked = ipfsOn(c.read);
  } catch (e) {
    note.textContent = `The mirror could not start: ${e?.message || e}`;
  }
}

/** The catalog as the reader asked: sorted (bump, new, replies; sticky first) and searched. */
function sorted(v) {
  const q = $('bd-search').value.trim().toLowerCase();
  const by = { bump: (t) => t.bump, new: (t) => t.no, replies: (t) => t.r }[$('bd-sort').value] || ((t) => t.bump);
  const catalog = v.catalog
    .filter((t) => !q || `${t.sub} ${t.ex}`.toLowerCase().includes(q))
    .sort((a, b) => (b.st - a.st) || (by(b) - by(a)) || (b.no - a.no));
  return { ...v, catalog };
}

// ---- kept across visits: the last catalog (B-UX-3), mirrors, the IPFS opt-ins ------------------

const store$ = {
  get(k) { try { return JSON.parse(localStorage.getItem(k) || 'null'); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* storage blocked or full */ } },
};

/** The last verified catalog of a board (no thread bodies), shown at once on the next visit. */
function cacheView(v) {
  store$.set(`ephem-board-view:${v.name}`, { ...v, threads: [] });
}
function cachedView(name) {
  const v = store$.get(`ephem-board-view:${name}`);
  if (v) views.set(name, v);
  return v;
}

function rememberMirror(name, onions) {
  const list = (store$.get('ephem-board-mirrors') || []).filter((m) => m.n !== name);
  list.push({ n: name, o: onions });
  store$.set('ephem-board-mirrors', list.slice(-8));
}

/** Mirrors this browser kept start again once Tor is up. */
async function resumeMirrors() {
  const list = store$.get('ephem-board-mirrors') || [];
  if (!list.length) return;
  const b = await net();
  for (const m of list) b.mirror(m.n, m.o.join(','), mirrorSeed(m.n)).catch((e) => console.info('board mirror:', e?.message || e));
}

const ipfsOn = (name) => !!name && (store$.get('ephem-board-ipfs') || []).includes(name);
function setIpfs(name, on) {
  if (!name) return;
  const list = (store$.get('ephem-board-ipfs') || []).filter((n) => n !== name);
  if (on) list.push(name);
  store$.set('ephem-board-ipfs', list);
  if (on) publishIpfs();
}

let ipfsAt = 0;
/** The opted-in boards' records (hosted or mirrored here) to the IPFS routing network, at most
 *  every 10 minutes (G.5.3), through a Tor exit. */
async function publishIpfs() {
  if (!boards || Date.now() - ipfsAt < 10 * 60_000) return;
  ipfsAt = Date.now();
  const [host, root] = channels.vaultApi.routing();
  for (const name of store$.get('ephem-board-ipfs') || []) {
    boards.publish_ipfs(name, host, root).catch((e) => console.info('board ipfs:', e?.message || e));
  }
}

// ---- the reply box: solves while you type (G.6.1 step 2) --------------------------------------

function setBox() {
  const t = current?.thread || 0;
  $('bd-box-title').textContent = t ? `Reply to No. ${t}` : 'New thread';
  $('bd-sub').hidden = !!t;
  $('bd-trip-row').hidden = !signedIn();
  $('bd-post-state').textContent = '';
  box = null;
  const key = draftKey();
  try { $('bd-body').value = sessionStorage.getItem(key) || ''; } catch { $('bd-body').value = ''; }
}

const draftKey = () => `ephem-board-draft:${current?.read}:${current?.thread || 0}`;

/** Starts the proof of work for the reply box now (on focus), so most posts wait for nothing. */
function presolve() {
  const c = current;
  if (!c?.read) return null;
  const trip = signedIn() ? $('bd-trip').value.trim() : '';
  const key = `${c.read}|${c.thread || 0}|${trip}`;
  if (box?.key === key) return box.promise;
  const state = $('bd-post-state');
  const promise = (async () => {
    const b = await net();
    const onion = c.onions[0];
    const draft = await b.draft(c.read, onion, c.thread || 0, trip);
    const sw = JSON.parse(draft.switches);
    if (draft.paused) throw new Error('E_BOARD_PAUSED: posting is paused on this board');
    if (!c.thread && !draft.threads_open) throw new Error('E_BOARD_PAUSED: new threads are closed on this board');
    if (sw.trips_only && !trip) state.textContent = 'This board accepts trips only right now.';
    const t0 = performance.now();
    const s = await solve(draft.params(), (n) => { state.textContent = `Preparing your post (proof of work, ${n} attempts)…`; });
    state.textContent = `Ready (${Math.round((performance.now() - t0) / 1000)} s of work).`;
    return { draft, s, premod: sw.premod };
  })();
  box = { key, promise };
  promise.catch(() => { if (box?.promise === promise) box = null; });
  return promise;
}

const REASONS = {
  E_BOARD_POW: 'The board asked for more work; try again.',
  E_BOARD_BUSY: 'The board is busy (or a thread was started moments ago); try again in a minute.',
  E_BOARD_REFUSED: 'Refused: the thread is locked or gone, the same text was just posted, or this key is banned.',
  E_BOARD_PAUSED: 'Posting is paused here, or limited to trips.',
  E_BOARD_OFFLINE: 'The board\'s host is offline: you can read from mirrors; your text is kept for later.',
};

async function submitPost(e) {
  e.preventDefault();
  const c = current;
  const state = $('bd-post-state');
  const body = $('bd-body').value;
  const sub = c.thread ? '' : $('bd-sub').value.trim();
  if (!body.trim() && !sub) return;
  $('b-bd-post').disabled = true;
  try {
    const { draft, s } = await presolve();
    const b = await app();
    box = null; // one solution, one post
    const r = JSON.parse(await b.post_draft(draft, sub, body, $('bd-sage').checked, s.n, s.solution));
    state.textContent = r.no ? `Posted as No. ${r.no}.` : 'Held for the owner\'s approval.';
    $('bd-body').value = '';
    $('bd-sub').value = '';
    try { sessionStorage.removeItem(draftKey()); } catch { /* nothing kept */ }
    if (!c.thread && r.no) current.thread = r.no;
    setTimeout(async () => {
      await refresh();
      const v = views.get(c.read);
      const seen = r.no && v?.threads.some((t) => t.posts.some((p) => p.no === r.no));
      if (seen) state.textContent = `✓ No. ${r.no} is on the board.`;
    }, 1_500);
    setBox();
    if (r.no) state.textContent = `Posted as No. ${r.no}.`;
  } catch (err) {
    const m = String(err?.message || err);
    const code = m.match(/E_BOARD_[A-Z]+/)?.[0];
    state.textContent = REASONS[code] || `Not posted: ${m}`;
    box = null;
  } finally {
    $('b-bd-post').disabled = false;
  }
}

// ---- proof of work ----------------------------------------------------------------------------

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
          else if (e.data.error) reject(new Error(`proof of work: ${e.data.error}`));
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

/** Opens a reply box, solves, signs and submits: resolves to `{no, seq, held, draft}` (`held`:
 *  pre-moderation, no number yet). `trip`: a label to post under the identity's trip key. */
export async function post(name, onion, thread, sub, body, sage, { trip = '', onProgress } = {}) {
  const b = await net();
  const draft = await b.draft(name, onion, thread, trip);
  const t0 = performance.now();
  const s = await solve(draft.params(), onProgress);
  const solveMs = performance.now() - t0;
  const r = JSON.parse(await b.post_draft(draft, sub, body, sage, s.n, s.solution));
  return { ...r, held: r.no === 0, trip: draft.trip, draft, solveMs, effort: draft.effort_now };
}

/** The same submit again (a dropped answer): the host returns the original number. */
export async function resend(draft) {
  return JSON.parse(await (await app()).resend(draft));
}

/** Reads and verifies board `name` with the given threads (numbers). */
export async function read(name, onions, threads = [], minSeq = 0) {
  return JSON.parse(await (await net()).read(name, onions, minSeq, threads.map(Number)));
}

// ---- wiring -----------------------------------------------------------------------------------

function wire() {
  $('b-board-new').onclick = showNew;
  $('b-ba-host').onclick = () => { if (current?.own !== undefined) takeOver(current.own); };
  $('b-ba-continue').onclick = () => { if (current?.own !== undefined) continueHere(current.own); };
  $('b-board-create').onclick = create;
  $('b-bo-copy').onclick = () => navigator.clipboard?.writeText($('bo-link').value);
  $('b-bo-open').onclick = () => {
    const o = owned.find((x) => x.i === current?.own);
    if (o) { ctx.setTab('follow'); showBoard(o.name, [JSON.parse(boards.status(o.i)).onion]); }
  };
  for (const k of ['paused', 'threads_closed', 'trips_only', 'approved_only', 'premod', 'panic_trips']) $(`bs-${k}`).onchange = setSwitches;
  $('b-bo-efforts').onclick = () => act(() => boards.set_efforts(current.own, Number($('bo-eff-reply').value), Number($('bo-eff-thread').value)));
  $('b-bo-mirrors').onclick = () => act(() => boards.set_mirrors(current.own, $('bo-mirrors').value));
  $('b-bo-see').onclick = () => act(() => boards.set_see_also(current.own, $('bo-see').value.replace(/\s+/g, '')));
  $('bo-ipfs').onchange = () => setIpfs(owned.find((o) => o.i === current?.own)?.name, $('bo-ipfs').checked);
  $('bd-mirror-ipfs').onchange = () => setIpfs(current?.read, $('bd-mirror-ipfs').checked);
  $('bd-sort').onchange = () => { const v = views.get(current?.read); if (v) renderBoard(v); };
  $('bd-search').oninput = () => { const v = views.get(current?.read); if (v) renderBoard(v); };
  $('b-bo-from').onclick = () => {
    const no = Number($('bo-from').value);
    if (no > 0) act(() => boards.delete_from(current.own, no));
  };
  $('f-bo-thread').onsubmit = (e) => {
    e.preventDefault();
    act(() => boards.post(current.own, 0, $('bo-sub').value, $('bo-body').value, false));
    $('bo-sub').value = '';
    $('bo-body').value = '';
  };
  $('b-bd-follow').onclick = follow;
  $('b-bd-unfollow').onclick = unfollow;
  $('b-bd-refresh').onclick = refresh;
  $('b-bd-catalog').onclick = () => { current.thread = 0; setBox(); const v = views.get(current.read); if (v) renderBoard(v); };
  $('b-bd-mirror').onclick = mirror;
  $('bd-body').onfocus = () => { presolve()?.catch((e) => { $('bd-post-state').textContent = REASONS[String(e?.message).match(/E_BOARD_[A-Z]+/)?.[0]] || String(e?.message || e); }); };
  $('bd-body').oninput = () => { try { sessionStorage.setItem(draftKey(), $('bd-body').value); } catch { /* not kept */ } };
  $('bd-trip').onchange = () => { box = null; };
  $('f-bd-post').onsubmit = submitPost;
}
