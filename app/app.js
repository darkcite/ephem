// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem UI glue. Rust (pkg/ephem_bg.wasm) owns every piece of protocol and chat state; this file
// renders the DOM and forwards input. Event contract: crates/wasm/src/lib.rs `ev` / `meta`.
// Re-entrancy rule: an ephemEvent handler never calls into `app` synchronously (use `later`).
// Tor mode (§28): tor.html (html data-mode="tor") loads the Tor build, pkg/ephem_tor*, instead;
// the direct page loads it only for its channel tabs (channels.js).
//
// Several chats at once (Appendix F.3): every event names its chat (meta CHAT); each chat has a
// `Chat` here (its state and its own copy of the chat views, #t-chat), and only the chat on
// screen is attached to the page. A call for a chat selects it first, in the same task:
// `A(c).send(…)` (network events may load another chat in Rust between two tasks).
import * as slots from './slots.js';
import * as bridges from './bridges.js';
import * as channels from './channels.js';
import { avatar } from './ui.js';

const TOR = document.documentElement.dataset.mode === 'tor';
let App, qr_svg_path, mod;

const EV = { CODE: 1, CONNECTED: 2, HELLO: 3, CHAT: 4, DELIVERED: 5, DEGRADED: 6, ALIVE: 7, PEER_HIDDEN: 8, CLOSED: 9, ERROR: 10,
  PROGRESS: 11, PATH: 12, SETTING: 13, EDITED: 14, DELETED: 15, EXPIRED: 16, READ: 17, TYPING: 18, SUSPENDED: 19,
  REACTION: 20, PEER_READY: 21, IDENTITY_SENT: 22, IDENTITY_RECEIVED: 23, ROOM: 24, ROOM_CLOSED: 25, TOR: 26, CARD: 27, REDIAL: 28 };
// Meta block offsets (crates/wasm/src/lib.rs `meta`).
const META = { TTL: 0, HAS_REPLY: 4, SENDER: 5, REPLY_SEQ: 8, RESUMED: 0, MEMBER: 16, CHAT: 17, LEN: 24 };
const PENDING = 0xff;           // member index of a joiner the owner has not admitted yet
const OWNER = 0;
const ROLE = ['owner', 'member', 'observer'];
const REACTIONS = ['👍', '❤️', '😂', '😮', '😢', '🙏'];
const FLAG_GROUP = 2;
const FLAG_TRANSFER = 4;
const FLAG_OBSERVER = 8;
const CONTACT_HAS_ONION = 2; // contact flags (crates/crypto/src/contacts.rs `cflags`)
const FRAG = { 1: 'i', 2: 'a', 3: 'r', 4: 'q', 5: 't', 6: 'k' };
const TTL_LABEL = { 5: '5 seconds', 30: '30 seconds', 60: '1 minute', 300: '5 minutes', 3600: '1 hour', 86400: '1 day' };
const TTL_SHORT = { 5: '5s', 30: '30s', 60: '1m', 300: '5m', 3600: '1h', 86400: '1d' };
// ErrorCode values (§19) for negative return values.
const ERR = { 0x11: 'E_ROOM_FULL', 0x12: 'E_ROOM_DISPOSED', 0x13: 'E_NOT_OWNER', 0x23: 'E_DUPLICATE_SESSION', 0x24: 'E_NOT_A_CONTACT', 0x01: 'E_INVALID_INVITE', 0x02: 'E_EXPIRED_INVITE', 0x03: 'E_INVITE_CONSUMED', 0x04: 'E_ANSWER_MISMATCH', 0x10: 'E_INVALID_ROOM',
  0x20: 'E_AUTH_FAILED', 0x21: 'E_CRYPTO_FAILED', 0x22: 'E_SAS_REJECTED', 0x30: 'E_ICE_FAILED', 0x31: 'E_NO_DIRECT_PATH', 0x32: 'E_RELAY_REJECTED',
  0x35: 'E_PEER_OFFLINE', 0x36: 'E_TOR_UNAVAILABLE', 0x40: 'E_PROTOCOL_MISMATCH', 0x41: 'E_MESSAGE_TOO_LARGE', 0x42: 'E_BACKPRESSURE', 0x43: 'E_NOT_PERMITTED', 0x44: 'E_TOO_MANY_CHATS', 0x60: 'E_KEYFILE_INVALID' };
const MESSAGES = {
  E_INVALID_INVITE: 'This is not a valid Ephem code. Copy the whole link again.',
  E_EXPIRED_INVITE: 'This code has expired. Ask for a new one.',
  E_INVITE_CONSUMED: 'This invite was already answered. Each invite connects one person.',
  E_ANSWER_MISMATCH: 'This answer does not belong to any invite open in this tab. Open it in the tab that created the invite.',
  E_INVALID_ROOM: 'This reconnect code belongs to a chat that is not open here.',
  E_AUTH_FAILED: 'This reconnect code was made by someone else, not by your peer.',
  E_PROTOCOL_MISMATCH: 'Your peer uses an incompatible version of Ephem.',
  E_CRYPTO_FAILED: 'The encrypted handshake failed. The codes may have been altered.',
  E_SAS_REJECTED: 'You reported that the safety codes differ. The chat was closed: the exchange may have been intercepted.',
  E_NO_DIRECT_PATH: 'No direct path between you and your peer. Ephem never uses a relay. Common causes: a VPN such as WARP, or strict NATs on both sides. Try another network (e.g. mobile data), or LAN only on the same Wi-Fi.',
  E_ICE_FAILED: 'The direct connection was lost.',
  E_TOR_UNAVAILABLE: 'Tor is not reachable from here (the Snowflake broker may be blocked on this network: try your own bridges in the settings). Tor mode never falls back to a direct connection.',
  E_RELAY_REJECTED: 'The connection went through a relay, which Ephem does not allow. The chat was closed.',
  E_PEER_OFFLINE: 'Your peer left the chat. Nothing was stored.',
  E_MESSAGE_TOO_LARGE: 'Message too long (max 4096 bytes).',
  E_BACKPRESSURE: 'Too many messages are waiting for your peer. Wait until they reconnect.',
  E_NOT_PERMITTED: 'That is not possible right now.',
  E_TOO_MANY_CHATS: 'This tab already holds 16 chats and rooms. Close one first.',
  E_KEYFILE_INVALID: 'Wrong passphrase, or the key file is damaged.',
  E_BROWSER_UNSUPPORTED: 'This browser does not support WebRTC data channels.',
  E_DUPLICATE_SESSION: 'This identity is already open in another tab. Close it there first.',
  E_NOT_A_CONTACT: 'That contact does not exist any more.',
  E_ROOM_FULL: 'The room is full (16 people).',
  E_ROOM_DISPOSED: 'The owner closed the room. Nothing was stored.',
  E_NOT_OWNER: 'Only the room owner can do that.',
};
// In a room, a member's link to the owner ending means the room is gone for it.
const ROOM_MESSAGES = { E_PEER_OFFLINE: 'E_ROOM_DISPOSED', E_NOT_PERMITTED: 'You were removed from the room. Nothing was stored.' };

const $ = (id) => document.getElementById(id);
const dec = new TextDecoder();
const enc = new TextEncoder();
let wasm, app;
let metaPtr = 0;               // event side-channel block (never moves: boxed at start)
let scanStop = null;
let torReady = false;
let lockRelease = null;        // releases the Web Lock of the saved identity in use (§7.2)
let updateWorker = null;

const later = (fn) => queueMicrotask(fn);
const mem = (ptr, len) => new Uint8Array(wasm.memory.buffer, ptr, len);
const text = (ptr, len) => dec.decode(mem(ptr, len));
const baseUrl = () => location.origin + location.pathname;
const keyOf = (sender, seq) => `${sender}:${seq}`;
const metaView = () => new DataView(wasm.memory.buffer, metaPtr, META.LEN);
const errName = (neg) => ERR[-neg] || `error 0x${(-neg).toString(16)}`;
const b64u = (u8) => btoa(String.fromCharCode(...u8)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const unb64u = (s) => {
  s = s.trim().replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(s + '==='.slice((s.length + 3) % 4)), (c) => c.charCodeAt(0));
};

// ---- chats -----------------------------------------------------------------------------------
const chats = new Map();       // chat id → Chat, in order of creation
const gone = new Map();        // chat id → ms: closed here; late events for it are dropped
let shown = null;              // the Chat on screen, or null

class Chat {
  constructor(id) {
    this.id = id;
    this.root = $('t-chat').content.firstElementChild.cloneNode(true);
    this.root.dataset.chat = id;
    this.title = 'New chat';
    this.label = '';              // status chip
    this.cls = '';
    this.preview = '';            // last message (RAM only, like the chat)
    this.unread = 0;
    this.at = Date.now();         // last activity (list order)
    this.open = false;            // a chat view is live (connected at least once)
    this.over = false;            // ended: only its notice is left
    this.codeExpires = 0;         // ms, for the countdown of the code on screen
    this.composing = null;        // { mode: 'reply' | 'edit', sender, seq }
    this.readSent = 0;
    this.typingTimer = 0;
    this.pathText = '';
    this.cardRequest = false;     // came through our contact card: ask first (§28.4)
    this.transferring = null;     // identity transfer (§7.6): 'receiver' | 'sender'
    this.xferDone = false;
    this.receivedBlob = null;     // the received key file, still passphrase-encrypted
    this.peerNick = '';           // what the peer calls itself (HELLO); never authentication
    this.myIdx = 0;               // our member index: messages are identified by (sender, seq)
    this.room = null;             // { owner, role, confirmed } while in a room (§14)
    this.names = new Map();       // room member index → display name
    this.deliveredBy = new Map(); // room member index → cumulative delivered seq of our messages
    this.removedByMe = new Set(); // owner: members just removed
    this.msgs = new Map();        // '<sender>:<seq>' → { li, body, tick, text, meta, sender, seq, mine }
    this.visibleTheirs = new Set();
    wire(this);
  }

  $(id) {
    return this.root.querySelector(`#${id}`);
  }

  show(view) {
    for (const v of this.root.querySelectorAll('.chatroot > .view')) v.hidden = v.id !== view;
    if (this === shown) $('error').hidden = true;
  }

  status(label, cls = '') {
    this.label = label;
    this.cls = cls;
    if (this === shown) setStatus(label, cls);
    renderList();
  }

  nameOf(idx) {
    return idx === this.myIdx ? 'You' : this.names.get(idx) || (this.room ? `member ${idx}` : 'Peer');
  }
}

/** `app` with chat `c` selected (for the call that follows, in the same task). */
function A(c) {
  app.select(c.id);
  return app;
}

/** The Chat of `id`, created for a chat Rust just started (by the user, or an incoming dial). */
function ensureChat(id) {
  let c = chats.get(id);
  if (c) return c;
  const closed = gone.get(id);
  if (closed && Date.now() - closed < 10_000) return null;
  gone.delete(id);
  c = new Chat(id);
  chats.set(id, c);
  return c;
}

/** Puts chat `c` on screen (the Chats tab, pane = the chat). */
function display(c) {
  if (shown && shown !== c) shown.root.remove();
  shown = c;
  $('chat-slot').replaceChildren(c.root);
  c.unread = 0;
  setTab('chats');
  showPane('chat-slot');
  setStatus(c.label, c.cls);
  renderList();
  later(() => flushRead(c));
}

/** Back to the Chats home (new chat, identity, contacts, settings). */
function home() {
  if (shown) shown.root.remove();
  shown = null;
  setTab('chats');
  showPane('v-start');
  setStatus(TOR ? (torReady ? 'Tor ready' : 'starting Tor') : 'ready', torReady ? 'ok' : '');
  renderIdentity();
  renderList();
}

/** Closes chat `c` for good (Rust wipes it) and drops its view. */
function closeChat(c) {
  gone.set(c.id, Date.now());
  chats.delete(c.id);
  later(() => { if (app.select(c.id)) app.close(); });
  if (shown === c) home();
  else renderList();
}

// ---- shell: tabs, lists, pane ------------------------------------------------------------------
let tab = 'chats';

function setTab(t) {
  tab = t;
  for (const b of document.querySelectorAll('.tabs [role=tab]')) b.setAttribute('aria-selected', String(b.dataset.tab === t));
  $('list-chats').hidden = t !== 'chats';
  $('list-follow').hidden = t !== 'follow';
  $('list-own').hidden = t !== 'own';
  $('list-settings').hidden = t !== 'settings';
}

/** The Settings tab (identity, contacts, connection, about), at section `id` if given. */
function openSettings(id) {
  setTab('settings');
  showPane('v-settings');
  // Phones: Settings has no list to go back to (its section list is the desktop sidebar).
  if (phone()) $('b-back').hidden = true;
  renderIdentity();
  if (id) $(id).scrollIntoView({ block: 'start' });
}

function showPane(id) {
  for (const v of document.querySelectorAll('#pane > .view')) v.hidden = v.id !== id;
  $('error').hidden = true;
  document.body.classList.add('pane-open');
  $('b-back').hidden = false;
}

/** The phone layout (one column: a tab's list, or a pane with Back). */
const phone = () => matchMedia('(max-width: 899px)').matches;

/** Phone layout: back from the pane to the list. */
function backToList() {
  document.body.classList.remove('pane-open');
  $('b-back').hidden = true;
}

function setStatus(label, cls = '') {
  $('status').textContent = label;
  $('status').className = 'pill status ' + cls;
}

function error(name) {
  $('error').textContent = MESSAGES[name] || name;
  $('error').hidden = false;
  $('error').title = 'Tap to dismiss';
}

function renderList() {
  const ul = $('chats');
  ul.replaceChildren();
  let unread = 0;
  for (const c of [...chats.values()].sort((a, b) => b.at - a.at)) {
    unread += c.unread;
    const li = document.createElement('li');
    li.dataset.chat = c.id;
    li.className = (c === shown ? 'active ' : '') + (c.cls || '');
    li.innerHTML = '<span class="dot"></span><span class="grow"><b></b><span class="sub"></span></span>';
    li.querySelector('b').textContent = c.title + (c.verified ? ' ✔' : '');
    li.querySelector('.sub').textContent = c.preview || c.label;
    avatar(li, c.title);
    if (c.unread) {
      const n = document.createElement('span');
      n.className = 'badge';
      n.textContent = String(c.unread);
      li.append(n);
    }
    li.onclick = () => display(c);
    ul.append(li);
  }
  $('chats-empty').hidden = chats.size > 0;
  // A contact with an open chat is shown by the chat's row. Deferred: the list is redrawn from
  // event handlers, while Rust still holds its state (reading the contacts there would panic).
  later(renderContacts);
  $('badge-chats').hidden = !unread;
  $('badge-chats').textContent = String(unread);
}

// System notifications while in the background: never the message text (P7: it would stay in
// the OS notification store).
const notifyOn = () => { try { return localStorage.getItem('ephem-notify') === '1'; } catch { return false; } };
function notify(title) {
  if (!notifyOn() || !document.hidden || !('Notification' in globalThis) || Notification.permission !== 'granted') return;
  try { new Notification(title, { tag: 'ephem', silent: false }); } catch { /* not in this context */ }
}

// In-app notices (docs/P2P-CHAT.md F.6): something happened out of view (another chat, a
// followed channel). One notice per source (a burst of messages updates its count), at most 3
// on screen, gone after 6 s or on a tap, which opens the source. Never the message text: only
// who and how many, as for system notifications (P7 applies to the screen too).
const NOTICE_MS = 6000;
const MAX_NOTICES = 3;
const notices = new Map();           // key → { el, n, timer }
function notice(key, title, sub, open) {
  let x = notices.get(key);
  if (!x) {
    const el = document.createElement('div');
    el.className = 'notice';
    el.setAttribute('role', 'status');
    el.innerHTML = '<span class="grow"><b></b><span class="sub"></span></span><button class="ghost" aria-label="Dismiss">×</button>';
    x = { el, n: 0, timer: 0 };
    notices.set(key, x);
    $('notices').prepend(el);
    while (notices.size > MAX_NOTICES) dropNotice(notices.keys().next().value);
  }
  x.n++;
  x.el.querySelector('b').textContent = title;
  x.el.querySelector('.sub').textContent = typeof sub === 'function' ? sub(x.n) : sub;
  x.el.onclick = (e) => { dropNotice(key); if (!e.target.closest('button')) open?.(); };
  clearTimeout(x.timer);
  x.timer = setTimeout(() => dropNotice(key), NOTICE_MS);
}
function dropNotice(key) {
  const x = notices.get(key);
  if (!x) return;
  clearTimeout(x.timer);
  x.el.remove();
  notices.delete(key);
}

/** Settings → About → Display details: what this device reports about the screen (layout bugs
 *  on phones differ per device and mode; these numbers say which). */
function renderDisplay() {
  const probe = (id) => {
    let el = document.getElementById(id);
    if (!el) { el = document.createElement('div'); el.id = id; document.body.append(el); }
    return el;
  };
  const cs = getComputedStyle(probe('safe-probe'));
  const h = (id) => Math.round(probe(id).getBoundingClientRect().height);
  const r = (sel) => { const e = document.querySelector(sel); if (!e) return '-'; const b = e.getBoundingClientRect(); return `${Math.round(b.top)}–${Math.round(b.bottom)}`; };
  const vv = window.visualViewport;
  $('disp-text').textContent = [
    `build            ${document.querySelector('meta[name="ephem-build"]')?.content}`,
    `mode             ${navigator.standalone ? 'home screen app' : matchMedia('(display-mode: standalone)').matches ? 'standalone' : 'browser'}`,
    `screen           ${screen.width} × ${screen.height} @${devicePixelRatio}`,
    `inner            ${innerWidth} × ${innerHeight}; outer ${outerWidth} × ${outerHeight}`,
    `visualViewport   ${vv ? `${Math.round(vv.width)} × ${Math.round(vv.height)} at ${Math.round(vv.offsetTop)}, scale ${vv.scale}` : '-'}`,
    `100dvh/vh/lvh    ${h('dvh-probe')} / ${h('vh-probe')} / ${h('lvh-probe')}`,
    `safe area t/b    ${cs.paddingTop} / ${cs.paddingBottom}`,
    `html / body      ${Math.round(document.documentElement.getBoundingClientRect().height)} / ${r('body')}`,
    `header / tabs    ${r('.bar')} / ${r('.tabs')}`,
    `scrollY          ${scrollY}; body.kbd ${document.body.classList.contains('kbd')}`,
  ].join('\n');
}

function renderCodeBox(box, kind, code) {
  const link = `${baseUrl()}#${FRAG[kind]}=${code}`;
  box.querySelector('.qr').innerHTML = qr_svg_path(link);
  box.querySelector('.link').value = link;
  box.querySelector('.share').hidden = !navigator.share;
  box.hidden = false;
}

function renderExposure(c) {
  const lines = [...new Set(A(c).exposure().trim().split('\n').filter(Boolean))]; // one IP may appear with several ports
  const pretty = lines.map((l) => (l.startsWith('v6') ? 'IPv6 ' : 'IPv4 ') + l.slice(3));
  const box = c.$('exposure');
  box.querySelector('.addrs').textContent = pretty.length ? pretty.join('\n') : 'No public address in this code (LAN only, or STUN was unreachable).';
  box.querySelector('.v6warn').hidden = !(lines.some((l) => l.startsWith('v4')) && lines.some((l) => l.startsWith('v6')));
  box.hidden = false;
  c.$('diag-exposure').textContent = pretty.length ? pretty.join(', ') : 'no public address';
}

// ---- messages ------------------------------------------------------------------------------
// The pane scrolls (the page never does; the composer is sticky): follow new messages only if the
// reader is at the bottom.
const atBottom = () => { const p = $('pane'); return p.scrollTop + p.clientHeight >= p.scrollHeight - 160; };
const toBottom = () => { const p = $('pane'); p.scrollTop = p.scrollHeight; };

/** The frame follows the visible viewport (Appendix F.3.1). On an iPhone the on-screen keyboard
 *  covers the bottom of the page and iOS scrolls the whole page up to show the focused field,
 *  pushing the header off-screen and leaving the tab bar floating over the keyboard. Instead the
 *  frame shrinks to the area above the keyboard (--vvh), the page is held at the top, and the
 *  tab bar is hidden while typing (body.kbd), so the composer sits right on the keyboard. */
function fitViewport() {
  const vv = window.visualViewport;
  if (!vv) return;
  const follow = atBottom();
  const field = document.activeElement?.matches?.('textarea, input:not([type=button]):not([type=checkbox]):not([type=radio]), select');
  const kbd = !!field && phone() && window.innerHeight - vv.height > 120;
  // Only while the keyboard is up: otherwise the frame is 100dvh. (An iPhone Home Screen app
  // reports a visual viewport shorter than the screen, which left a strip under the tab bar.)
  if (kbd) document.documentElement.style.setProperty('--vvh', `${Math.round(vv.height)}px`);
  else document.documentElement.style.removeProperty('--vvh');
  document.body.classList.toggle('kbd', kbd);
  // Only with the keyboard: iOS then slides the page up to show the field.
  if (kbd && (window.scrollY || vv.offsetTop)) window.scrollTo(0, 0);
  if (follow || (kbd && document.activeElement.closest('.composer'))) toBottom();
}
// Installed as a Home Screen app: its own tab bar size (app.css html.standalone).
if (navigator.standalone === true || matchMedia('(display-mode: standalone)').matches) document.documentElement.classList.add('standalone');
if (window.visualViewport) {
  visualViewport.addEventListener('resize', fitViewport);
  visualViewport.addEventListener('scroll', fitViewport);
  document.addEventListener('focusin', () => setTimeout(fitViewport, 50));
  document.addEventListener('focusout', () => setTimeout(fitViewport, 50));
}

function sysLine(c, t) {
  const follow = c === shown && atBottom();
  const li = document.createElement('li');
  li.className = 'sys';
  li.textContent = t;
  c.$('log').append(li);
  if (follow) toBottom();
}

function quoteText(c, sender, seq) {
  const m = c.msgs.get(keyOf(sender, seq));
  if (!m || m.deleted) return 'Message unavailable';
  return `${c.nameOf(sender)}: ${m.text.slice(0, 80)}`;
}

function addMessage(c, sender, seq, body, ttl, reply) {
  const mine = sender === c.myIdx;
  const li = document.createElement('li');
  li.className = mine ? 'me' : 'them';
  li.dataset.key = keyOf(sender, seq);
  if (c.room && !mine) {
    const who = document.createElement('span');
    who.className = 'who';
    who.textContent = c.nameOf(sender);
    li.append(who);
  }
  if (reply) {
    const q = document.createElement('span');
    q.className = 'quote';
    q.dataset.ref = keyOf(reply.sender, reply.seq);
    q.textContent = quoteText(c, reply.sender, reply.seq);
    li.append(q);
  }
  const b = document.createElement('span');
  b.className = 'body';
  b.textContent = body;
  li.append(b);
  const meta = document.createElement('span');
  meta.className = 'meta';
  meta.textContent = ttl ? `⏱ ${TTL_SHORT[ttl] || ttl + 's'}` : '';
  li.append(meta);
  let tick = null;
  if (mine) {
    tick = document.createElement('span');
    tick.className = 'tick';
    tick.textContent = '🕓';
    tick.title = 'Pending';
    li.append(tick);
  }
  const reacts = document.createElement('span');
  reacts.className = 'reacts';
  li.append(reacts);
  const m = { li, body: b, tick, meta, reacts, text: body, sender, mine, seq, deleted: false, level: 0, reactions: new Map() };
  c.msgs.set(li.dataset.key, m);
  const follow = c === shown && (mine || atBottom());
  c.$('log').append(li);
  if (follow) toBottom();
  if (!mine && !c.room) io.observe(li);
  if (mine && c.room) roomTick(c, m);
  c.preview = `${mine ? 'You: ' : c.room ? c.nameOf(sender) + ': ' : ''}${body.slice(0, 60)}`;
  c.at = Date.now();
  if (!mine && (c !== shown || tab !== 'chats' || document.hidden)) {
    if (c !== shown) c.unread++;
    if (c !== shown || tab !== 'chats') {
      notice(`chat:${c.id}`, c.room ? `${c.nameOf(sender)} in ${c.title}` : c.title, (n) => (n === 1 ? 'New message' : `${n} new messages`), () => display(c));
    }
    notify(`New message in Ephem${c.room ? ' (room)' : ''}`);
  }
  renderList();
  return m;
}

// Room delivery: "✓ k/N", N = the other members now in the room (§14.3).
function roomTick(c, m) {
  const others = [...c.names.keys()].filter((i) => i !== c.myIdx);
  const k = others.filter((i) => (c.deliveredBy.get(i) || 0) >= m.seq).length;
  m.tick.textContent = others.length ? `✓ ${k}/${others.length}` : '🕓';
  m.tick.title = others.length ? `Delivered to ${k} of ${others.length}` : 'Nobody else in the room yet';
  m.tick.classList.toggle('ok', others.length > 0 && k === others.length);
}

function setTick(c, upto, level) {
  for (const m of c.msgs.values()) {
    if (!m.mine || m.seq > upto || m.level >= level || m.deleted) continue;
    m.level = level;
    m.tick.textContent = level === 1 ? '✓' : '✓✓';
    m.tick.title = level === 1 ? 'Delivered' : 'Read';
    m.tick.classList.add('ok');
  }
}

// Re-renders every quote of message `key` (after an edit, a delete or an expiry).
function refreshQuotes(c, key) {
  const [sender, seq] = key.split(':').map(Number);
  for (const q of c.root.querySelectorAll(`.quote[data-ref="${key}"]`)) q.textContent = quoteText(c, sender, seq);
}

function markDeleted(c, key, label) {
  const m = c.msgs.get(key);
  if (!m) return;
  m.deleted = true;
  m.text = '';
  m.li.classList.add('deleted');
  m.body.textContent = label;
  m.meta.textContent = '';
  m.li.querySelector('.acts')?.remove();
  refreshQuotes(c, key);
}

function removeMessage(c, key) {
  const m = c.msgs.get(key);
  if (!m) return;
  io.unobserve(m.li);
  m.li.remove();
  c.msgs.delete(key);
  refreshQuotes(c, key);
}

function toggleActions(c, m) {
  const old = m.li.querySelector('.acts');
  for (const a of c.root.querySelectorAll('.log .acts')) a.remove();
  if (old || m.deleted) return;
  const acts = document.createElement('div');
  acts.className = 'acts';
  const add = (label, fn) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = label;
    b.onclick = (e) => { e.stopPropagation(); acts.remove(); fn(); };
    acts.append(b);
  };
  const observer = c.room?.role === 2;
  if (!observer) {
    add('Reply', () => startComposing(c, 'reply', m));
    add('React', () => showPicker(c, m));
  }
  if (m.mine) add('Edit', () => startComposing(c, 'edit', m));
  if (c.room?.owner && !m.mine) add('Delete for everyone', () => deleteMessage(c, m, true));
  add(m.mine ? 'Delete for everyone' : 'Delete for me', () => deleteMessage(c, m, false));
  add('Copy', () => navigator.clipboard?.writeText(m.text).catch(() => {}));
  m.li.append(acts);
}

// One reaction per person per message; the latest wins, empty removes (§11.7).
function renderReactions(c, m) {
  m.reacts.replaceChildren();
  for (const [by, e] of m.reactions) {
    if (!e) continue;
    const t = document.createElement('span');
    t.textContent = `${e} ${by === c.myIdx ? 'you' : c.room ? c.nameOf(by) : 'peer'}`;
    m.reacts.append(t);
  }
}

function showPicker(c, m) {
  const picker = document.createElement('div');
  picker.className = 'picker';
  for (const e of [...REACTIONS, '✕']) {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = e;
    b.title = e === '✕' ? 'Remove my reaction' : 'React';
    b.onclick = (ev) => {
      ev.stopPropagation();
      picker.remove();
      const emoji = e === '✕' ? '' : e;
      const n = writeText(emoji);
      const r = A(c).react(m.sender, m.seq, n);
      if (r < 0) return error(errName(r));
      m.reactions.set(c.myIdx, emoji);
      renderReactions(c, m);
    };
    picker.append(b);
  }
  m.li.append(picker);
}

// "Anything that renders as more than one grapheme is rejected" (§11.7).
const oneGrapheme = (s) => !s || !globalThis.Intl?.Segmenter || [...new Intl.Segmenter().segment(s)].length === 1;

function startComposing(c, mode, m) {
  c.composing = { mode, sender: m.sender, seq: m.seq };
  c.$('composing-text').textContent = mode === 'edit' ? 'Editing your message' : 'Reply to ' + quoteText(c, m.sender, m.seq);
  c.$('composing').hidden = false;
  if (mode === 'edit') c.$('t-msg').value = m.text;
  c.$('t-msg').focus();
}

function stopComposing(c) {
  c.composing = null;
  c.$('composing').hidden = true;
}

// Ours: for everyone. Someone else's: for me only, or for everyone by the room owner.
function deleteMessage(c, m, moderate) {
  if (!m.mine && !moderate) {
    if (!c.room?.owner) A(c).delete(m.sender, m.seq); // local only: drops its self-destruct timer (an owner's call would moderate)
    return removeMessage(c, m.li.dataset.key);
  }
  const r = A(c).delete(m.sender, m.seq);
  if (r < 0) return error(errName(r));
  markDeleted(c, m.li.dataset.key, m.mine ? 'You deleted this message' : 'You removed this message');
}

// Read receipts: a message counts as read when it is on screen and the page is visible (§11.7).
function flushRead(c) {
  if (!c || c !== shown || document.hidden || !c.visibleTheirs.size) return;
  const max = Math.max(...c.visibleTheirs);
  if (max > c.readSent) {
    c.readSent = max;
    A(c).mark_read(max);
  }
}

// ---- events from Rust ----------------------------------------------------------------------
// Every event names its chat (meta CHAT) and the link it came from (meta MEMBER: the peer's
// member index). In a 1:1 chat there is one link; in a room the member's link to the owner plays
// that part (the "primary").
const primary = (c, idx) => !c.room || (!c.room.owner && idx === OWNER);

globalThis.ephemEvent = (kind, num, ptr, len) => {
  const mv = metaView();
  // Events of the tab, not of a chat.
  if (kind === EV.TOR) return torEvent(num, text(ptr, len));
  if (kind === EV.ERROR) return error(text(ptr, len));
  if (kind === EV.CARD && num === 2) return later(persist);
  const c = ensureChat(mv.getUint8(META.CHAT));
  if (!c) return;
  // A chat that started without the user (a contact or card dial): on screen if nothing else
  // is (the Chats home, or a chat that ended).
  const free = () => (!shown && !$('v-start').hidden) || (shown?.over && !$('chat-slot').hidden);
  if (c !== shown && free() && (kind === EV.CONNECTED || kind === EV.CARD)) later(() => { if (free() && chats.has(c.id)) display(c); });
  const from = mv.getUint8(META.MEMBER);
  switch (kind) {
    case EV.CODE: {
      const code = text(ptr, len);
      if (c.room?.owner && (num === 1 || num === 5)) {
        showRoomInvite(c, num, code);
        break;
      }
      c.codeExpires = num === 1 || num === 3 || num === 5 ? Date.now() + Number($('s-ttl').value) * 1000 : 0;
      if (num <= 2 || num === 5) showCode(c, num, code);
      else showResumeCode(c, num, code);
      later(() => renderExposure(c));
      break;
    }
    case EV.PROGRESS:
      if (primary(c, from)) c.status(['', 'gathering', 'connecting', 'handshake'][num] || '');
      break;
    case EV.CONNECTED: {
      const b = mem(ptr, len);
      const resumed = mv.getUint8(META.RESUMED) === 1;
      if (!primary(c, from)) {
        if (c.room.owner && from === PENDING) {
          hideRoomInvite(c);
          sysLine(c, 'Someone answered your invite. Admitting them to the room…');
        }
        later(() => renderRoom(c));
        break;
      }
      c.status('connected', 'ok');
      c.codeExpires = 0;
      if (resumed) {
        c.$('resume').hidden = true;
        c.$('b-redial').hidden = true;
        c.$('peer-state').textContent = '';
        sysLine(c, `Reconnected ${TOR ? 'through Tor' : 'directly'}. Pending messages are being delivered.`);
        break;
      }
      const d = String(num).padStart(6, '0');
      const digits = d.slice(0, 3) + ' ' + d.slice(3);
      const emoji = Array.from(b.subarray(0, 4), (x) => String.fromCodePoint(0x1f400 + x) + '️').join(' ');
      const handle = dec.decode(b.subarray(4));
      c.peerNick = '';
      if (c.transferring) {
        later(() => showTransfer(c, digits, emoji));
        break;
      }
      if (!c.room) c.myIdx = from === 0 ? 1 : 0; // 1:1: the offerer is 0, the answerer 1
      c.$('sas-digits').textContent = digits;
      c.$('sas-emoji').textContent = emoji;
      c.$('sas-help').textContent = c.room
        ? 'Compare it with the room owner on another channel, for example a call. It proves the room (and its member list) comes from them.'
        : 'Compare it with your peer on another channel, for example a call. If it differs, someone may be in the middle.';
      c.$('peer').textContent = handle;
      c.$('peer').dataset.handle = handle;
      c.title = c.room ? `Room of ${handle}` : handle;
      c.$('imp-warn').hidden = true;
      c.$('sas').hidden = false;
      c.$('sas').classList.remove('optional');
      c.$('verified').textContent = 'unverified';
      c.$('verified').className = 'pill';
      if (c.cardRequest) later(() => showCardRequest(c));
      openChat(c, c.room ? 'Connected to the room owner. The member list arrives next.'
        : TOR ? 'Connected through Tor: neither of you sees the other\'s IP address. Messages are end-to-end encrypted and exist only in these two tabs.'
          : 'Connected directly. Messages are end-to-end encrypted and exist only in these two tabs.');
      if (c !== shown) {
        notify('A chat connected in Ephem');
        notice(`chat:${c.id}`, c.title || 'A chat', 'Connected', () => display(c));
      }
      break;
    }
    case EV.HELLO: {
      if (c.transferring) break;
      if (!primary(c, from)) {
        later(() => renderRoom(c));
        break;
      }
      c.peerNick = text(ptr, len);
      if (c.cardRequest) later(() => showCardRequest(c));
      later(() => fillContactNick(c));
      // Both codes scanned in person: the SAS is shown but not prompted (§10.4).
      if (num === 1 && c.$('verified').textContent === 'unverified') {
        c.$('sas').classList.add('optional');
        c.$('verified').textContent = 'met in person';
      }
      later(() => renderPeer(c));
      if (c.room) later(() => renderRoom(c));
      break;
    }
    case EV.REACTION: {
      const m = c.msgs.get(keyOf(mv.getUint8(META.SENDER), num));
      const e = text(ptr, len);
      if (m && !m.deleted && oneGrapheme(e)) {
        m.reactions.set(from, e);
        renderReactions(c, m);
      }
      break;
    }
    case EV.PEER_READY:
      c.$('xfer-state').textContent = 'The new device confirmed the code.';
      break;
    case EV.IDENTITY_SENT:
      c.xferDone = true;
      c.$('xfer-state').textContent = '';
      c.$('xfer-sent').hidden = false;
      later(() => { if (app.select(c.id)) app.close(); });
      break;
    case EV.IDENTITY_RECEIVED:
      c.xferDone = true;
      c.receivedBlob = mem(ptr, len).slice(); // documented copy: the key file leaves wasm to be kept by JS until unlocked
      c.$('xfer-state').textContent = '';
      c.$('xfer-unlock').hidden = false;
      later(() => { if (app.select(c.id)) app.close(); c.$('i-xfer-pass').focus(); });
      break;
    case EV.CHAT: {
      const ttl = mv.getUint32(META.TTL, true);
      const reply = mv.getUint8(META.HAS_REPLY) ? { sender: mv.getUint8(META.SENDER), seq: mv.getFloat64(META.REPLY_SEQ, true) } : null;
      addMessage(c, from, num, text(ptr, len), ttl, reply);
      if (!c.room) c.$('peer-state').textContent = '';
      break;
    }
    case EV.DELIVERED:
      if (!c.room) {
        setTick(c, num, 1);
        break;
      }
      c.deliveredBy.set(from, Math.max(num, c.deliveredBy.get(from) || 0));
      for (const m of c.msgs.values()) if (m.mine && !m.deleted && m.seq <= num) roomTick(c, m);
      break;
    case EV.READ:
      setTick(c, num, 2);
      break;
    case EV.SETTING:
      c.$('s-chat-ttl').value = String(num);
      if (c.room) sysLine(c, num ? `The owner set messages to disappear after ${TTL_LABEL[num]}.` : 'The owner turned off disappearing messages.');
      else sysLine(c, num ? `Your peer set messages to disappear after ${TTL_LABEL[num]}.` : 'Your peer turned off disappearing messages.');
      break;
    case EV.EDITED: {
      const m = c.msgs.get(keyOf(from, num));
      if (m && !m.deleted) {
        m.text = text(ptr, len);
        m.body.textContent = m.text;
        m.meta.textContent = 'edited';
        refreshQuotes(c, m.li.dataset.key);
      }
      break;
    }
    case EV.DELETED: {
      const sender = mv.getUint8(META.SENDER);
      markDeleted(c, keyOf(sender, num), sender !== from && from === OWNER ? 'Removed by the room owner' : 'Message deleted');
      break;
    }
    case EV.EXPIRED:
      removeMessage(c, keyOf(mv.getUint8(META.SENDER), num));
      break;
    case EV.TYPING:
      c.$('peer-state').textContent = num ? 'typing…' : '';
      break;
    case EV.DEGRADED:
      if (!primary(c, from)) break;
      c.status('no response', 'bad');
      c.$('peer-state').textContent = c.room ? 'owner not responding…' : 'connection problem…';
      break;
    case EV.ALIVE:
      if (!primary(c, from)) break;
      c.status('connected', 'ok');
      c.$('peer-state').textContent = '';
      c.$('b-redial').hidden = true;
      break;
    case EV.PEER_HIDDEN:
      if (!c.room) c.$('peer-state').textContent = num ? 'in background' : '';
      break;
    case EV.PATH:
      if (!primary(c, from)) break;
      c.pathText = text(ptr, len);
      renderPath(c);
      break;
    case EV.SUSPENDED:
      if (!primary(c, from)) {
        later(() => renderRoom(c));
        break;
      }
      c.status('disconnected', 'bad');
      c.$('peer-state').textContent = c.room ? 'owner unreachable' : '';
      if (TOR) {
        // No reconnect codes over Tor: whoever dialled dials the onion again (§28.5); the host
        // waits for it. In a room the member dials its owner.
        const dialler = c.room ? !c.room.owner : c.myIdx === 1;
        if (c.open) sysLine(c, dialler ? 'The Tor connection dropped. Reconnecting through Tor…' : 'The Tor connection dropped. Waiting for your peer to reconnect: their Ephem dials yours again (their tab must be open).');
        c.$('peer-state').textContent = dialler ? 'reconnecting…' : 'waiting for your peer…';
        c.$('b-redial').hidden = !dialler;
      } else if (c.open) {
        c.$('resume').hidden = false;
        c.$('resume').querySelector('.codebox').hidden = true;
        sysLine(c, c.room ? 'Direct path to the room owner lost. Share a reconnect code with the owner to continue.' : 'Direct path lost. Share a reconnect code to continue.');
      }
      break;
    case EV.CLOSED: {
      const name = text(ptr, len);
      if (c.room && !primary(c, from)) {
        later(() => renderRoom(c));
        break;
      }
      c.status('closed', 'bad');
      if (!c.xferDone) later(() => ended(c, c.room ? ROOM_MESSAGES[name] || name : name));
      break;
    }
    case EV.ROOM:
      later(() => renderRoom(c));
      break;
    case EV.ROOM_CLOSED: {
      const name = text(ptr, len);
      if (c.room) later(() => ended(c, ROOM_MESSAGES[name] || name));
      break;
    }
    case EV.REDIAL: {
      // Attempt `num` to reach the peer's onion again; `text`: why the last one failed.
      const why = text(ptr, len);
      if (c.open) c.$('peer-state').textContent = `reconnecting… attempt ${num}${why ? ` (last: ${why.slice(0, 80)})` : ''}`;
      break;
    }
    case EV.CARD:
      c.cardRequest = true;
      c.title = 'From your contact card';
      later(() => showCardRequest(c));
      notify('Someone wants to connect through your contact card');
      if (c !== shown) notice(`chat:${c.id}`, 'Someone has your contact card', 'Wants to connect: tap to answer', () => display(c));
      break;
  }
};

// Our onion is fully reachable once arti has its descriptor on both HSDir rings (current and next
// time period); until then peers whose clock or consensus picks the other ring cannot reach it.
let torOnion = '';
function renderReach() {
  if (!torOnion) return;
  const r = app.tor_reach();
  const at = `${torOnion.slice(0, 8)}….onion`;
  const full = r === 'reachable' || r === 'degraded';
  $('tor-state').textContent = `Reachable through Tor while this tab is open (${at}): by your invites, and by your contacts when you are signed in.${full ? '' : ' Tor is still publishing your address: some peers may not reach you for a minute or two.'}`;
}

function torEvent(num, t) {
  if (num === 2) {
    torReady = true;
    later(saveTorCache);
    later(() => channels.torReady());
    torOnion = t;
    renderReach();
    // The chip shows the chat on screen; anywhere else, Tor's state.
    if (!shown) setStatus('Tor ready', 'ok');
  } else if (num === 3) {
    $('tor-state').textContent = `Tor failed: ${t}`;
    if (!shown) setStatus('Tor failed', 'bad');
    error('E_TOR_UNAVAILABLE');
  }
}

// ---- views of a chat ---------------------------------------------------------------------------
function showCode(c, kind, code) {
  c.$('code-title').textContent = c.transferring === 'receiver' ? 'Receive an identity' : kind === 1 || kind === 5 ? 'Your invite' : 'Your answer';
  c.$('code-help').textContent = c.transferring === 'receiver'
    ? 'On the old device, sign in with the identity you want to move, then scan this code (or open the link) and send back the answer.'
    : c.transferring === 'sender'
      ? 'Show this answer to the new device (QR or link). Both devices then show a safety code to compare.'
      : kind === 5
        ? 'Let your peer scan this QR code, or send them the link (they open it in Tor mode). It works once, and no answer is needed: keep this tab open until they connect through Tor.'
        : kind === 1
          ? 'Let your peer scan this QR code, or send them the link. It works once.'
          : 'Send this answer back to the person who invited you (QR or link). The chat opens as soon as they apply it.';
  renderCodeBox(c.root.querySelector('#v-code .codebox'), kind, code);
  c.$('answer-box').hidden = kind !== 1;
  c.title = c.transferring ? 'Identity transfer' : kind === 1 || kind === 5 ? 'Invite (waiting)' : 'Answer (waiting)';
  c.status(kind === 1 ? 'waiting for answer' : 'waiting for peer');
  c.show('v-code');
}

function showResumeCode(c, kind, code) {
  const box = c.$('resume').querySelector('.codebox');
  box.querySelector('.resume-help').textContent = kind === 3
    ? 'Send this reconnect code to your peer. Then scan or paste their answer below.'
    : 'Send this answer back to your peer. The chat reconnects as soon as they apply it.';
  renderCodeBox(box, kind, code);
  c.$('resume').hidden = false;
  c.status(kind === 3 ? 'waiting for answer' : 'waiting for peer');
}

// The chat view, fresh: a 1:1 chat or a room (member: once its owner link is up).
function openChat(c, line) {
  c.$('log').replaceChildren();
  c.msgs.clear();
  c.visibleTheirs.clear();
  c.deliveredBy.clear();
  c.readSent = 0;
  stopComposing(c);
  c.$('resume').hidden = true;
  c.$('diag').hidden = true;
  c.$('s-chat-ttl').value = '0';
  // Rooms: only the owner sets the timer; observers only read (§11.7).
  c.$('s-chat-ttl').disabled = !!c.room && !c.room.owner;
  c.$('f-send').hidden = c.room?.role === 2;
  c.$('room').hidden = !c.room;
  c.open = true;
  sysLine(c, line);
  c.show('v-chat');
  renderList();
  later(() => { renderPeer(c); if (c.room) renderRoom(c); if (c === shown) c.$('t-msg').focus(); });
}

// A used or withdrawn invite must never be picked up again.
function hideRoomInvite(c) {
  const box = c.$('room-invite');
  box.hidden = true;
  box.querySelector('.link').value = '';
  box.querySelector('.qr').replaceChildren();
}

function showRoomInvite(c, kind, code) {
  renderCodeBox(c.$('room-invite').querySelector('.codebox'), kind, code);
  c.$('t-room-answer').value = '';
  c.$('room-invite').hidden = false;
}

// Members, their roles and our direct link to each (§14); the owner can remove members.
function renderRoom(c) {
  if (!c.room || !chats.has(c.id)) return;
  const [me, role, , version, confirmed] = A(c).room_info().split('\t').map(Number);
  if (Number.isNaN(me)) return;
  c.myIdx = me;
  c.room.role = role;
  c.room.confirmed = confirmed === 1;
  const rows = A(c).room_members().split('\n').filter(Boolean).map((l) => l.split('\t'));
  const before = new Map(c.names);
  c.names.clear();
  for (const [idx, , handle, , nick] of rows) c.names.set(Number(idx), nick ? `${nick} (${handle})` : handle);
  // Departures, whatever the path (left, removed, link lost for good), from the signed state.
  for (const [idx, name] of before) if (!c.names.has(idx) && !c.removedByMe.delete(idx)) sysLine(c, `${name} is no longer in the room.`);
  const ul = c.$('members');
  ul.replaceChildren();
  const LINK = { me: 'you', connected: TOR ? 'via Tor' : 'direct', connecting: 'connecting…', suspended: 'reconnecting…', 'no-path': 'no direct path', none: 'not connected' };
  for (const [idx, r, , link, , sas] of rows) {
    const i = Number(idx);
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'grow';
    name.innerHTML = '<b></b> <span class="role"></span> <span></span> <span class="dim"></span>';
    name.querySelector('b').textContent = i === c.myIdx ? c.names.get(i) + ' (you)' : c.names.get(i);
    name.querySelector('.role').textContent = ROLE[Number(r)];
    const st = name.children[2];
    st.textContent = link === 'me' ? '' : LINK[link] || link;
    st.className = link === 'connected' ? 'link-ok' : link === 'no-path' || link === 'suspended' ? 'link-bad' : 'dim';
    // The owner compares each member's safety code with that member (§10.4).
    if (c.room.owner && i !== c.myIdx && sas !== '0') name.querySelector('.dim').textContent = `SAS ${sas.padStart(6, '0').replace(/(\d{3})(\d{3})/, '$1 $2')}`;
    li.append(name);
    if (c.room.owner && i !== OWNER) {
      const rm = document.createElement('button');
      rm.textContent = 'Remove';
      rm.onclick = () => {
        if (!confirm(`Remove ${c.names.get(i)} from the room?`)) return;
        c.removedByMe.add(i);
        if (A(c).room_remove(i) === 0) sysLine(c, `You removed ${c.names.get(i)}.`);
        else c.removedByMe.delete(i);
      };
      li.append(rm);
    }
    ul.append(li);
  }
  c.$('room-count').textContent = `${rows.length} / 16`;
  c.$('room-role').textContent = ROLE[role] || '';
  c.$('room-owner').hidden = !c.room.owner;
  c.$('f-send').hidden = role === 2;
  const others = rows.length - 2;
  // Joined as the only member: the invite prompt already said every future member sees our IP.
  if (!c.room.owner && !c.room.confirmed && version > 0 && others <= 0) {
    c.room.confirmed = true;
    A(c).room_connect();
  }
  c.$('room-confirm').hidden = c.room.owner || c.room.confirmed || version === 0 || others <= 0;
  c.$('room-confirm-text').textContent = `Connect directly to ${others} other member${others === 1 ? '' : 's'}? Each of them will see your IP address, and you theirs (Ephem never uses a relay).`;
  for (const m of c.msgs.values()) if (m.mine && !m.deleted) roomTick(c, m);
  c.title = c.room.owner ? `Your room (${rows.length})` : `${c.$('peer').textContent} (${rows.length})`;
  renderList();
}

// Contact name, verification and the impersonation warning in the chat header (§7.5).
function renderPeer(c) {
  if (!chats.has(c.id)) return;
  if (c.room) {
    c.$('b-save-contact').hidden = true;
    c.$('imp-warn').hidden = true;
    if (!c.room.owner) c.$('peer').textContent = `Room of ${c.$('peer').dataset.handle || 'the owner'}${c.peerNick ? ` “${c.peerNick}”` : ''}`;
    return;
  }
  const [flags, nick] = (A(c).peer_contact() || '').split('\t');
  const handle = c.$('peer').dataset.handle || '';
  const contact = flags !== undefined && flags !== '';
  const verified = contact && (Number(flags) & 1) === 1;
  c.$('peer').textContent = contact ? `${nick || handle}${verified ? ' ✔' : ''}` : c.peerNick ? `${handle} “${c.peerNick}”` : handle;
  c.$('peer').title = contact ? `Contact ${handle}` : c.peerNick ? 'The name in quotes is chosen by the peer, not verified' : '';
  c.title = contact ? nick || handle : c.peerNick ? `${c.peerNick} (${handle})` : handle;
  c.verified = verified;
  c.$('b-save-contact').hidden = !app.identity_label() || contact;
  if (verified) {
    // The key is already pinned by a verified contact: no SAS prompt (§10.4).
    c.$('sas').hidden = true;
    c.$('verified').textContent = 'verified contact';
    c.$('verified').className = 'pill ok';
  }
  const imp = c.peerNick ? A(c).impersonates(c.peerNick) : '';
  c.$('imp-warn').hidden = !imp;
  c.$('imp-warn').textContent = imp ? `This is not the “${imp}” you verified: the name matches but the key is different. Compare the safety code.` : '';
  renderList();
}

function showTransfer(c, digits, emoji) {
  c.$('xfer-digits').textContent = digits;
  c.$('xfer-emoji').textContent = emoji;
  c.$('xfer-title').textContent = c.transferring === 'receiver' ? 'Receive an identity' : `Send identity “${app.identity_label()}”`;
  c.$('xfer-help').textContent = c.transferring === 'receiver'
    ? 'Compare the safety code with the old device. After both confirm, the old device sends its encrypted key file.'
    : 'Compare the safety code with the new device. Only confirm if both show the same code and the other device is yours.';
  c.$('xfer-sas-actions').hidden = false;
  c.$('xfer-state').textContent = '';
  c.$('xfer-unlock').hidden = true;
  c.$('xfer-sent').hidden = true;
  c.status('connected', 'ok');
  c.show('v-transfer');
}

function endTransfer(c) {
  c.receivedBlob = null;
  c.$('i-xfer-pass').value = '';
  closeChat(c);
}

function ended(c, name) {
  if (!chats.has(c.id)) return;
  c.transferring = null;
  c.cardRequest = false;
  c.over = true;
  c.$('card-req').hidden = true;
  c.$('log').hidden = false;
  const wasChat = c.open;
  const wasRoom = !!c.room;
  c.open = false;
  c.room = null;
  c.names.clear();
  c.msgs.clear();
  c.visibleTheirs.clear();
  c.$('log').replaceChildren();
  c.$('note-title').textContent = wasChat ? (wasRoom ? 'Room closed' : 'Chat ended') : 'Could not connect';
  c.$('note-text').textContent = MESSAGES[name] || name;
  c.$('b-again').textContent = 'Close';
  c.$('b-again').hidden = false;
  c.preview = wasChat ? (wasRoom ? 'Room closed' : 'Chat ended') : 'Could not connect';
  c.status('closed', 'bad');
  c.show('v-note');
}

// The UI never shows raw IP addresses unless "show addresses" is on (§18).
function renderPath(c) {
  c.$('diag-path').textContent = c.$('c-addr').checked
    ? c.pathText
    : c.pathText.replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, '•••').replace(/\[[0-9a-fA-F:.]+\]/g, '[•••]');
}

// ---- the controls of one chat (its copy of #t-chat) -------------------------------------------
function wire(c) {
  const on = (id, fn) => { c.$(id).onclick = fn; };
  for (const box of c.root.querySelectorAll('.codebox')) {
    box.querySelector('.copy').onclick = () => copyLink(box);
    box.querySelector('.share').onclick = () => navigator.share({ url: box.querySelector('.link').value }).catch(() => {});
  }
  on('b-answer', () => applyCode(c.$('t-answer').value, false));
  on('b-scan-answer', () => scan((t) => applyCode(t, true)));
  on('b-cancel', () => closeChat(c));
  on('b-again', () => closeChat(c));
  on('b-leave', () => {
    if (c.room?.owner && c.names.size > 1 && !confirm('Close the room for everyone?')) return;
    const note = c.room ? (c.room.owner ? 'You closed the room. Nothing was stored.' : 'You left the room. Nothing was stored.') : 'You left the chat. Nothing was stored.';
    later(() => { if (app.select(c.id)) app.close(); });
    ended(c, 'E_PEER_OFFLINE');
    c.$('note-text').textContent = note;
  });
  on('b-room-invite', () => { hideRoomInvite(c); applyPrefs(); A(c).room_invite(false, Number($('s-ttl').value)); });
  on('b-room-observer', () => { hideRoomInvite(c); applyPrefs(); A(c).room_invite(true, Number($('s-ttl').value)); });
  on('b-room-answer', () => applyCode(c.$('t-room-answer').value, false));
  on('b-scan-room', () => scan((t) => applyCode(t, true)));
  on('b-room-connect', () => { A(c).room_connect(); renderRoom(c); });
  on('b-room-decline', () => c.$('b-leave').click());
  on('b-sas-ok', () => {
    c.$('sas').hidden = true;
    c.$('verified').textContent = 'verified';
    c.$('verified').className = 'pill ok';
    A(c).confirm_sas();
    if (A(c).peer_contact()) persist().then(() => renderPeer(c));
  });
  on('b-sas-bad', () => {
    later(() => { if (app.select(c.id)) app.close(); });
    ended(c, 'E_SAS_REJECTED');
  });
  // Saved under the name they gave themselves; rename it in the contact's pane (Chats → Contacts).
  on('b-save-contact', () => {
    if (A(c).save_contact(c.peerNick || '') === 0) persist().then(() => renderPeer(c));
  });
  on('b-card-accept', () => answerCardRequest(c, true));
  on('b-card-decline', () => answerCardRequest(c, false));
  on('b-xfer-ok', () => {
    if (A(c).confirm_sas() !== 0) return;
    c.$('xfer-sas-actions').hidden = true;
    c.$('xfer-state').textContent = c.transferring === 'receiver' ? 'Waiting for the identity…' : 'Waiting for the new device to confirm…';
  });
  on('b-xfer-bad', () => {
    later(() => { if (app.select(c.id)) app.close(); });
    ended(c, 'E_SAS_REJECTED');
  });
  on('b-xfer-unlock', () => unlockTransferred(c));
  on('b-xfer-keep', () => endTransfer(c));
  on('b-xfer-remove', async () => {
    if (!confirm('Remove this identity from this device? Make sure the other device unlocked it.')) return;
    await slots.remove(app.lock_name());
    app.new_temporary_identity();
    lockIdentity();
    endTransfer(c);
  });
  on('b-restart', () => app.restart_ice());
  on('b-info', () => { c.$('diag').hidden = !c.$('diag').hidden; if (!c.$('diag').hidden) renderExposure(c); });
  on('b-drop', () => A(c).drop_path());
  on('b-redial', () => { app.tor_redial_now(); c.$('peer-state').textContent = 'reconnecting now…'; });
  c.$('c-addr').onchange = () => renderPath(c);
  on('b-resume', () => A(c).create_resume(Number($('s-ttl').value)));
  on('b-scan-resume', () => scan((t) => applyCode(t, true)));
  on('b-resume-apply', () => { applyCode(c.$('t-resume').value, false); c.$('t-resume').value = ''; });
  on('b-composing-x', () => { if (c.composing?.mode === 'edit') c.$('t-msg').value = ''; stopComposing(c); });
  c.$('s-chat-ttl').onchange = () => setChatTtl(c);
  c.$('log').onclick = (e) => {
    const li = e.target.closest('li[data-key]');
    const m = li && c.msgs.get(li.dataset.key);
    if (m) toggleActions(c, m);
  };
  for (const b of c.root.querySelectorAll('.drop-v6')) {
    b.onclick = () => { $('c-v6').checked = true; applyPrefs(); b.closest('.v6warn').textContent = 'IPv6 will be left out of your next codes.'; };
  }
  c.$('f-send').onsubmit = (e) => send(e, c);
  c.$('t-msg').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) send(e, c);
    else if (e.key === 'Escape') stopComposing(c);
  });
  c.$('t-msg').addEventListener('input', () => onTyping(c));
}

// One saved identity per tab: a Web Lock named after the key (§7.2). Temporary identities are
// unique by construction and need none.
async function lockIdentity() {
  lockRelease?.();
  lockRelease = null;
  if (!navigator.locks || !app.identity_label()) return true;
  const name = app.lock_name();
  return new Promise((resolve) => {
    navigator.locks.request(name, { ifAvailable: true }, (lock) => {
      if (!lock) return resolve(false);
      resolve(true);
      return new Promise((release) => { lockRelease = release; });
    });
  });
}

function renderIdentity() {
  const label = app.identity_label();
  const h = app.handle();
  $('me').textContent = h;
  $('id-desc').textContent = label
    ? `Saved identity “${label}” (${h}). Peers see the same identity every time you use it.`
    : `Temporary identity ${h}. It disappears when you close this tab.`;
  $('b-id-temp').hidden = !label;
  $('card-reset').hidden = !label;
  if (document.activeElement !== $('i-nick')) $('i-nick').value = app.nick();
  renderContacts();
  renderSlots();
  bridgesOnIdentity();
  channels.onIdentity();
}

// ---- remembered identities (§7.2) and contacts (§7.5) ---------------------------------------
async function renderSlots() {
  const list = await slots.list();
  const current = app.identity_label() ? app.lock_name() : '';
  const ul = $('slots');
  ul.replaceChildren();
  ul.hidden = !list.length;
  for (const slot of list) {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'grow';
    name.innerHTML = '<b></b> <span class="dim"></span>';
    name.querySelector('b').textContent = slot.label || '(no label)';
    name.querySelector('.dim').textContent = slot.handle + (slot.id === current ? ' · in use' : '');
    li.append(name);
    if (slot.id !== current) {
      const pass = document.createElement('input');
      pass.type = 'password';
      pass.placeholder = 'passphrase';
      pass.autocomplete = 'current-password';
      pass.hidden = true;
      const go = document.createElement('button');
      go.textContent = 'Sign in';
      go.onclick = async () => {
        if (pass.hidden) { pass.hidden = false; pass.focus(); return; }
        const pw = enc.encode(pass.value);
        pass.value = '';
        await signIn(slot.blob, pw, false);
      };
      pass.onkeydown = (e) => { if (e.key === 'Enter') go.click(); };
      li.append(pass, go);
    }
    const forget = document.createElement('button');
    forget.textContent = 'Forget';
    forget.title = 'Remove from this device (your key files are not affected)';
    forget.onclick = async () => {
      if (!confirm(`Forget “${slot.label}” on this device? Without a downloaded key file it is gone for good.`)) return;
      await slots.remove(slot.id);
      renderSlots();
    };
    li.append(forget);
    ul.append(li);
  }
  const mine = list.find((x) => x.id === current);
  $('backup-stale').hidden = !mine?.stale;
}

// ---- contacts: people in the Chats tab (docs/CONTACTS-UX.md) ---------------------------------
// One row per person: a contact with an open chat is shown by its chat row. A row opens the
// person pane; its button does the primary action (Tor: connect; otherwise: invite).
const UNDO_MS = 8000;
let removing = null;                 // { hex, name, timer }: a removal that can still be undone
let person = null;                   // the contact whose pane is open (hex)

/** Contacts as [{ hex, flags, nick, handle, name, verified, onion }] (none for a temporary identity). */
function contactRows() {
  if (!app.identity_label()) return [];
  return app.contacts().split('\n').filter(Boolean).map((l) => {
    const [hex, flags, nick, handle] = l.split('\t');
    return { hex, flags: Number(flags), nick, handle, name: nick || handle, verified: (Number(flags) & 1) === 1, onion: (Number(flags) & CONTACT_HAS_ONION) !== 0 };
  }).filter((r) => r.hex !== removing?.hex);
}

function renderContacts() {
  const saved = !!app.identity_label();
  const rows = contactRows();
  const inChat = new Set([...chats.values()].filter((c) => !c.room).map((c) => c.$('peer')?.dataset.handle).filter(Boolean));
  const ul = $('contacts');
  ul.replaceChildren();
  for (const r of rows) {
    if (inChat.has(r.handle)) continue;
    const li = document.createElement('li');
    li.dataset.hex = r.hex;
    li.innerHTML = '<span class="dot"></span><span class="grow"><b></b><span class="sub"></span></span><button class="act"></button>';
    li.querySelector('b').textContent = `${r.name}${r.verified ? ' ✔' : ''}`;
    avatar(li, r.name);
    const tor = TOR && r.onion;
    li.querySelector('.sub').textContent = tor ? 'Connect through Tor' : TOR ? 'Invite needed (no Tor address)' : 'Invite to chat';
    const act = li.querySelector('.act');
    act.textContent = tor ? 'Connect' : 'Invite';
    act.onclick = (e) => { e.stopPropagation(); contactAction(r); };
    li.onclick = () => showPerson(r.hex);
    if (person === r.hex && !$('v-person').hidden) li.classList.add('active');
    ul.append(li);
  }
  $('contacts-note').replaceChildren();
  if (!saved) {
    $('contacts-note').append('Contacts are kept in a saved identity. ');
    const b = document.createElement('button');
    b.textContent = 'Save your identity';
    b.onclick = () => openSettings('identity-card');
    $('contacts-note').append(b);
  } else if (!rows.length) {
    $('contacts-note').textContent = 'No contacts yet. Add one from their contact card (＋ New → Add a contact), or with “Add to contacts” in a chat.';
  }
  $('contacts-note').hidden = saved && rows.length > 0;
  $('share-strip').hidden = !saved || rows.length >= 3;
  if (person && !$('v-person').hidden) renderPerson();
  filterPeople();
}

/** The search field appears once there are 8 rows or more; it filters chats and contacts. */
function filterPeople() {
  const rows = [...document.querySelectorAll('#chats li, #contacts li')];
  $('i-people').hidden = rows.length < 8 && !$('i-people').value;
  const q = $('i-people').value.trim().toLowerCase();
  for (const li of rows) li.hidden = !!q && !li.textContent.toLowerCase().includes(q);
}

function contactAction(r) {
  if (TOR && r.onion) return connectContact(r.hex, r.name);
  // An invite: the chat then recognises the pinned key (name and ✔).
  home();
  $('b-invite').click();
  setStatus(`invite for ${r.name}: send it to them`);
}

function showPerson(hex) {
  person = hex;
  if (shown) shown.root.remove();
  shown = null;
  setTab('chats');
  showPane('v-person');
  renderPerson();
  renderList();
  renderContacts();
}

function renderPerson() {
  const r = contactRows().find((x) => x.hex === person);
  if (!r) {
    person = null;
    return home();
  }
  if (document.activeElement !== $('p-name')) $('p-name').value = r.nick;
  $('p-name').placeholder = r.handle;
  $('p-avatar').className = 'dot';
  avatar($('p-avatar').parentElement, r.name);
  $('p-verified').textContent = r.verified ? 'verified ✔' : 'not verified';
  $('p-verified').className = 'pill' + (r.verified ? ' ok' : '');
  $('p-handle').textContent = `${r.handle} · the handle comes from their key; the name above is yours for them.`;
  const tor = TOR && r.onion;
  $('b-p-go').textContent = tor ? 'Connect through Tor' : 'Invite to a chat';
  $('p-mode').textContent = tor ? 'Their Ephem must be open in Tor mode. No code is needed.'
    : TOR ? 'This contact was added without a Tor address: send them an invite; after one Tor chat, “Connect” works.'
      : 'Direct mode: a chat still needs an invite (two codes). The contact pins their key and name, so the chat shows who it is.';
  $('p-who').textContent = r.name;
  $('p-fp').textContent = app.contact_fingerprint(r.hex);
  $('b-p-verify').hidden = r.verified;
}

function removeContact() {
  const r = contactRows().find((x) => x.hex === person);
  if (!r) return;
  finishRemoval();
  removing = { hex: r.hex, name: r.name, timer: setTimeout(finishRemoval, UNDO_MS) };
  person = null;
  $('toast-text').textContent = `${r.name} removed.`;
  $('toast').hidden = false;
  home();
  renderContacts();
}

/** The removal becomes final (the undo window closed, or another one starts). */
function finishRemoval() {
  if (!removing) return;
  clearTimeout(removing.timer);
  const { hex } = removing;
  removing = null;
  $('toast').hidden = true;
  if (app.remove_contact(hex) === 0) persist();
  else renderContacts();
}

function undoRemoval() {
  if (!removing) return;
  clearTimeout(removing.timer);
  removing = null;
  $('toast').hidden = true;
  renderContacts();
}

/** ＋ New → Add a contact: paste or scan a card, and share ours. */
function showAdd(text = '') {
  if (shown) shown.root.remove();
  shown = null;
  setTab('chats');
  showPane('v-add');
  const saved = !!app.identity_label();
  $('card').hidden = !saved;
  $('card-none').hidden = saved;
  if (saved) renderCard();
  if (text) {
    $('t-card').value = text;
    $('i-card-name').value = app.card_nick(text) || '';
    $('i-card-name').focus();
  }
}

// After a change of contacts, nickname or settings: re-encrypt (the file key stays in wasm
// memory, §7.3), update the remembered slot, and flag the downloaded backup as out of date.
async function persist() {
  renderIdentity();
  const blob = app.resave_identity();
  if (!blob.length) return;
  const slot = await slots.get(app.lock_name());
  if (slot) await slots.put({ ...slot, blob, stale: true });
  $('backup-stale').hidden = false;
}

async function remember(blob) {
  const ok = await slots.put({ id: app.lock_name(), label: app.identity_label(), handle: app.handle(), blob, stale: false });
  if (!ok) error(`All ${slots.MAX_SLOTS} identity slots on this device are used. Forget one first; the key file still works.`);
}

// Signs in with an encrypted key file; one identity per tab (Web Lock, §7.2).
async function signIn(blob, pw, rememberIt) {
  const r = app.load_identity(blob, pw);
  if (r === 0x43) {
    const open = [...chats.values()].filter((c) => !c.over).length;
    error(`Close your open chats and rooms first (${open} open): the identity can change only while none is open.`);
    return false;
  }
  if (r !== 0) return false; // wrong passphrase or damaged file: the ERROR event said which
  if (!(await lockIdentity())) {
    app.new_temporary_identity();
    renderIdentity();
    error('E_DUPLICATE_SESSION');
    return false;
  }
  if (rememberIt) await remember(blob);
  renderIdentity();
  if (!shown) setStatus(`signed in: ${app.identity_label()}`, 'ok');
  return true;
}

// ---- actions -------------------------------------------------------------------------------
function applyPrefs() {
  app.set_prefs(Number($('s-privacy').value), $('c-v6').checked, $('c-read').checked, $('c-typing').checked);
}

/** After a call that started a conversation: its Chat, on screen. */
function started() {
  const c = ensureChat(app.chat());
  if (c) display(c);
  return c;
}

/** The "Got a code?" sheet (header button), from every tab. */
function openCodeSheet(text = '') {
  if (text) $('t-code').value = text;
  $('code-sheet').hidden = false;
  $('t-code').focus();
}
const closeCodeSheet = () => { $('code-sheet').hidden = true; };

function applyCode(raw, scanned) {
  const v = raw.trim();
  if (!v) return;
  closeCodeSheet();
  if (app.card_nick(v) !== undefined) return showAdd(v);
  if (/#c=/.test(v)) return channels.openLink(v);
  const info = app.code_info(v);
  let sending = false;
  if ((info & 0xff) === 1 && (info >> 8) & FLAG_TRANSFER) {
    // Someone asks for this identity (§7.6).
    if (!app.identity_label()) return error('Sign in with the identity you want to move first, then open this code again.');
    if (!confirm(`This code asks for your identity “${app.identity_label()}”. Only continue if the other device is yours. Continue?`)) return;
    sending = true;
  }
  const kind = info & 0xff;
  const group = (kind === 1 || kind === 5) && (info >> 8) & FLAG_GROUP;
  if (group) {
    // Direct rooms connect everyone directly: every member sees every other member's IP
    // (§29.2). Over Tor nobody sees anyone's IP.
    const observer = (info >> 8) & FLAG_OBSERVER;
    const as = observer ? ', as a read-only observer' : '';
    if (!confirm(kind === 5 ? `This is an invite to a room${as}, through Tor. Join?`
      : `This is an invite to a room${as}. Every member of the room will see your IP address, and you theirs (direct connections, never a relay). Join?`)) return;
  }
  applyPrefs();
  if (app.apply_code(v, scanned) !== 0) return;
  // Answers and reconnect codes go to the chat that made the invite (Rust finds it).
  if (kind !== 1 && kind !== 5) return;
  const c = started();
  if (!c) return;
  c.transferring = sending ? 'sender' : null;
  c.room = group ? { owner: false, role: (info >> 8) & FLAG_OBSERVER ? 2 : 1, confirmed: kind === 5 } : null;
  c.myIdx = 1;
  c.title = group ? 'Room (joining)' : 'Chat (connecting)';
  if (kind === 5) {
    c.status('connecting via Tor');
    c.$('note-title').textContent = 'Connecting through Tor…';
    c.$('note-text').textContent = 'Reaching your peer\'s onion service. This usually takes 10–60 seconds; their tab must be open.';
    c.$('b-again').textContent = 'Cancel';
    c.show('v-note');
  }
  $('t-code').value = '';
}

async function copyLink(box) {
  const link = box.querySelector('.link').value;
  const btn = box.querySelector('.copy');
  try {
    await navigator.clipboard.writeText(link);
    btn.textContent = 'Copied';
    setTimeout(() => (btn.textContent = 'Copy link'), 1500);
    // Best-effort clipboard clear after 60 s (§8.7).
    setTimeout(() => navigator.clipboard.writeText('').catch(() => {}), 60000);
  } catch {
    box.querySelector('.link').select();
  }
}

function writeText(msg) {
  // Zero-copy on our side: UTF-8 goes straight into the wasm text slot (§11.6).
  const { read, written } = enc.encodeInto(msg, mem(app.text_ptr(), app.text_cap()));
  return read < msg.length ? -1 : written;
}

function send(ev, c) {
  ev.preventDefault();
  const box = c.$('t-msg');
  const msg = box.value;
  if (!msg.trim()) return;
  const n = writeText(msg);
  if (n < 0) return error('E_MESSAGE_TOO_LARGE');
  if (c.composing?.mode === 'edit') {
    const r = A(c).edit(c.composing.seq, n);
    if (r < 0) return error(errName(r));
    const m = c.msgs.get(keyOf(c.myIdx, c.composing.seq));
    if (m) {
      m.text = msg;
      m.body.textContent = msg;
      m.meta.textContent = 'edited';
      refreshQuotes(c, m.li.dataset.key);
    }
  } else {
    const reply = c.composing?.mode === 'reply' ? c.composing : null;
    const seq = A(c).send(n, reply?.sender ?? 0, reply?.seq ?? 0);
    if (seq < 0) return error(errName(seq));
    addMessage(c, c.myIdx, seq, msg, A(c).chat_ttl(), reply && { sender: reply.sender, seq: reply.seq });
  }
  stopComposing(c);
  box.value = '';
  box.focus();
  clearTimeout(c.typingTimer);
}

function onTyping(c) {
  if (!c.open) return;
  A(c).typing(true);
  clearTimeout(c.typingTimer);
  c.typingTimer = setTimeout(() => { if (chats.has(c.id)) A(c).typing(false); }, 4000);
}

function setChatTtl(c) {
  const v = Number(c.$('s-chat-ttl').value);
  const r = A(c).set_ttl(v);
  if (r < 0) {
    c.$('s-chat-ttl').value = String(A(c).chat_ttl());
    return error(errName(r));
  }
  sysLine(c, v ? `You set messages to disappear after ${TTL_LABEL[v]}.` : 'You turned off disappearing messages.');
}

// ---- identity (§7.2, §7.3) -----------------------------------------------------------------
function download(bytes, name) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
}

function saveIdentity() {
  const label = $('i-label').value.trim();
  const p1 = $('i-pass').value;
  if ([...p1].length < 12) return error('The passphrase must be at least 12 characters.');
  if (p1 !== $('i-pass2').value) return error('The passphrases differ.');
  const pw = enc.encode(p1);
  $('i-pass').value = $('i-pass2').value = '';
  let blob = app.save_identity(label, pw); // pw is wiped by Rust
  if (!blob.length) return;
  // Settings kept under the temporary identity move into the new key file.
  let moved = false;
  if (ramBridges && app.set_section(0x05, ramBridges) === 0) {
    ramBridges = '';
    moved = true;
  }
  moved = channels.moveToKeyFile() || moved;
  if (moved) blob = app.resave_identity();
  download(blob, keyFileName());
  $('t-keytext').value = b64u(blob);
  $('id-saved').hidden = false;
  lockIdentity();
  if ($('c-remember').checked) remember(blob).then(renderIdentity);
  renderIdentity();
}

const keyFileName = () => `ephem-${app.identity_label().replace(/[^\w-]+/g, '_') || 'identity'}.p2pkey`;

async function downloadBackup() {
  const blob = app.resave_identity();
  if (!blob.length) return;
  download(blob, keyFileName());
  const slot = await slots.get(app.lock_name());
  if (slot) await slots.put({ ...slot, blob, stale: false });
  $('backup-stale').hidden = true;
}

async function loadIdentity() {
  let bytes;
  const f = $('i-file').files[0];
  try {
    bytes = f ? new Uint8Array(await f.arrayBuffer()) : unb64u($('t-keyin').value);
  } catch {
    return error('E_KEYFILE_INVALID');
  }
  const pw = enc.encode($('i-pass-in').value);
  $('i-pass-in').value = '';
  if (await signIn(bytes, pw, $('c-remember-in').checked)) {
    $('id-load').hidden = true;
    $('i-file').value = '';
    $('t-keyin').value = '';
  }
}

async function unlockTransferred(c) {
  const pw = enc.encode(c.$('i-xfer-pass').value);
  c.$('i-xfer-pass').value = '';
  if (!c.receivedBlob || !(await signIn(c.receivedBlob, pw, c.$('c-xfer-remember').checked))) return;
  if (confirm('Signed in. Download a backup of the key file now?')) download(c.receivedBlob, keyFileName());
  endTransfer(c);
}

// ---- QR scanner (§8.2): BarcodeDetector where it really supports QR, else rqrr in wasm (iOS) --
async function qrDetector() {
  // iOS can expose BarcodeDetector without QR support; trust it only if it lists qr_code.
  if (!('BarcodeDetector' in globalThis)) return null;
  try {
    const formats = await globalThis.BarcodeDetector.getSupportedFormats();
    return formats.includes('qr_code') ? new globalThis.BarcodeDetector({ formats: ['qr_code'] }) : null;
  } catch {
    return null;
  }
}

// One frame through the wasm decoder. Even frames: the whole picture (long side ≤ 1280 px);
// odd frames: a centre square at native resolution, which doubles the pixels per QR module.
function wasmScan(video, ctx, n) {
  const [vw, vh] = [video.videoWidth, video.videoHeight];
  let sx = 0, sy = 0, sw = vw, sh = vh;
  if (n % 2) {
    sw = sh = Math.round(Math.min(vw, vh) * 0.7);
    sx = Math.round((vw - sw) / 2);
    sy = Math.round((vh - sh) / 2);
  }
  const k = Math.min(1, 1280 / Math.max(sw, sh));
  const w = Math.round(sw * k);
  const h = Math.round(sh * k);
  ctx.canvas.width = w;
  ctx.canvas.height = h;
  ctx.drawImage(video, sx, sy, sw, sh, 0, 0, w, h);
  const img = ctx.getImageData(0, 0, w, h);
  // One documented copy (§11.6): camera frame → preallocated wasm scan buffer.
  const ptr = app.scan_buf(img.data.length);
  mem(ptr, img.data.length).set(img.data);
  return { text: app.scan(w, h), size: `${w}×${h}` };
}

async function scan(onText) {
  const video = $('scan-video');
  $('scanner').hidden = false;
  $('scan-status').textContent = 'Starting the camera…';
  let stream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: 'environment', width: { ideal: 1920 }, height: { ideal: 1080 } },
      audio: false,
    });
  } catch {
    $('scanner').hidden = true;
    return error('The camera is not available. Paste the code instead.');
  }
  let active = true;
  scanStop = () => {
    active = false;
    for (const t of stream.getTracks()) t.stop();
    video.srcObject = null;
    $('scanner').hidden = true;
    scanStop = null;
  };
  video.srcObject = stream;
  await video.play().catch(() => {});
  let detector = await qrDetector();
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  let frames = 0;
  while (active) {
    await new Promise((r) => setTimeout(r, 150));
    if (!active || !video.videoWidth) continue;
    const t0 = performance.now();
    let found = '';
    let info = '';
    if (detector) {
      try {
        found = (await detector.detect(video))[0]?.rawValue || '';
        info = 'native detector';
      } catch {
        detector = null; // broken detector: fall back to wasm for good
      }
    }
    if (!found && !detector) {
      const r = wasmScan(video, ctx, frames);
      found = r.text;
      info = `wasm ${r.size}`;
    }
    frames++;
    $('scan-status').textContent = `Looking for a code… ${frames} frames · ${info} · ${Math.round(performance.now() - t0)} ms`;
    if (found && /#[iarqtkbc]=/.test(found)) {
      scanStop();
      onText(found);
    } else if (found) {
      $('scan-status').textContent = 'That QR code is not an Ephem code.';
    }
  }
}

// ---- codes arriving by link (§8.7) ---------------------------------------------------------
const bc = 'BroadcastChannel' in globalThis ? new BroadcastChannel('p2pchat-codes') : null;

function takeFragment() {
  const h = location.hash;
  if (!/^#([iarqtkbc]=|tab=)/.test(h)) return null;
  history.replaceState(null, '', location.pathname);
  return h;
}

// Answers and reconnect codes belong to the tab that holds the chat: hand them over.
function forward(code) {
  return new Promise((resolve) => {
    if (!bc) return resolve(false);
    const t = setTimeout(() => resolve(false), 500);
    bc.onmessage = (e) => {
      if (e.data?.ack === code) {
        clearTimeout(t);
        resolve(true);
      }
    };
    bc.postMessage({ code });
  });
}

if (bc) {
  bc.addEventListener('message', (e) => {
    const code = e.data?.code;
    if (typeof code !== 'string' || !app || !app.code_fits(code)) return;
    bc.postMessage({ ack: code });
    applyCode(code, false);
  });
}

// ---- version pinning (§17.1): a new build waits until the user agrees ------------------------
async function registerWorker() {
  if (!('serviceWorker' in navigator)) return;
  const reg = await navigator.serviceWorker.register('sw.js', { updateViaCache: 'none' }).catch(() => null);
  if (!reg) return;
  const offer = (w) => {
    if (!w || !navigator.serviceWorker.controller) return; // first install: nothing to replace
    updateWorker = w;
    navigator.serviceWorker.addEventListener('message', (e) => {
      if (e.source === w && e.data?.version) $('update-text').textContent = `A new version of Ephem is available (build ${e.data.version}).`;
    });
    $('update-text').textContent = 'A new version of Ephem is available.';
    w.postMessage('version');
    $('update').hidden = false;
  };
  offer(reg.waiting);
  reg.addEventListener('updatefound', () => {
    const w = reg.installing;
    w?.addEventListener('statechange', () => { if (w.state === 'installed') offer(w); });
  });
  let reloading = false;
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (updateWorker && !reloading) {
      reloading = true;
      location.reload();
    }
  });
  document.addEventListener('visibilitychange', () => { if (!document.hidden) reg.update().catch(() => {}); });
}

function applyUpdate() {
  const live = [...chats.values()].some((c) => c.open);
  if (live && !confirm('Updating reloads Ephem and ends your open chats. Update now?')) return;
  updateWorker?.postMessage('activate');
}

// ---- contact cards (§7.5) ----------------------------------------------------------------------
function renderCard() {
  const code = app.my_card(false, Number($('s-card-ttl').value));
  if (!code) return;
  renderCodeBox($('card').querySelector('.codebox'), 6, code);
  const exp = app.card_expires();
  $('card-expiry').textContent = exp ? `This card works until ${new Date(exp * 1000).toLocaleDateString()}.` : 'This card never expires.';
}

function addCard(text, name) {
  if (!app.identity_label()) return error('Sign in with a saved identity first: contacts live in its key file.');
  if (app.add_card(text, name ?? app.card_nick(text) ?? '') !== 0) return;
  persist();
  $('t-code').value = '';
  $('t-card').value = '';
  $('i-card-name').value = '';
  setStatus(TOR ? 'contact added: Connect to chat' : 'contact added');
  home();
}

// The chat came through our card from someone who is not a contact yet (§28.4 case 3).
function showCardRequest(c) {
  if (!c.cardRequest || !c.open) return;
  c.$('card-req-text').textContent = `${c.peerNick || c.$('peer').dataset.handle || 'Someone'} (from your contact card) wants to connect. Compare the safety code, then accept or decline.`;
  c.$('card-req').hidden = false;
  // Nothing of the chat is shown before the user accepts.
  c.$('f-send').hidden = true;
  c.$('log').hidden = true;
}

/** A contact saved before the peer's HELLO has no name yet: give it the nickname it sent. */
function fillContactNick(c) {
  const handle = c.$('peer')?.dataset.handle;
  if (!c.peerNick || !handle || !app.identity_label()) return;
  const row = app.contacts().split('\n').map((l) => l.split('\t')).find((r) => r[3] === handle);
  if (row && !row[2] && app.rename_contact(row[0], c.peerNick) === 0) persist();
}

function answerCardRequest(c, accept) {
  c.cardRequest = false;
  c.$('card-req').hidden = true;
  if (!accept) return c.$('b-leave').click();
  c.$('f-send').hidden = false;
  c.$('log').hidden = false;
  // Their nickname may not have arrived yet (the request can come before HELLO over a slow
  // Tor path): saved without one, it is filled in when HELLO comes (fillContactNick).
  if (A(c).save_contact(c.peerNick || '') === 0) persist().then(() => renderPeer(c));
}

// ---- Tor mode (§28) --------------------------------------------------------------------------
function connectContact(hex, name) {
  if (app.contact_connect(hex) !== 0) return;
  const c = started();
  if (!c) return;
  c.myIdx = 1;
  c.title = name;
  c.status('connecting via Tor');
  c.$('note-title').textContent = `Connecting to ${name} through Tor…`;
  c.$('note-text').textContent = 'Their Ephem must be open in Tor mode and signed in. This usually takes 10–60 seconds.';
  c.$('b-again').textContent = 'Cancel';
  c.show('v-note');
}

// Test hooks, set before the page loads (a page script cannot set them: the CSP allows only our
// files): `ephemTorLab`, the offline lab's bridge lines, NAT hint and Tor network
// (checks/tor-lab; `bridges: null` = use the settings as a user would); `ephemTorLog`, an arti
// log level for the console (diagnostics of live runs).
let torStarted = false, torCustom = false, ramBridges = '';
function beginTor() {
  const lab = globalThis.ephemTorLab;
  if (lab?.bridges) return startTor(lab.bridges, false);
  if (bridges.waiting()) {
    $('tor-state').textContent = 'Tor is waiting for your own bridges: sign in with your identity, or paste them under “Connection and privacy settings” → Tor connection.';
    setStatus('Tor waits for bridges');
    return;
  }
  startTor(bridges.DEFAULT_BRIDGES, false);
}

async function startTor(lines, custom) {
  if (torStarted) return;
  const lab = globalThis.ephemTorLab;
  const log = lab?.log || globalThis.ephemTorLog;
  if (log) app.tor_log(log);
  const check = JSON.parse(app.bridges_check(lines));
  if (!check.usable) {
    $('tor-state').textContent = 'Your bridges cannot be used: fix them under “Connection and privacy settings” → Tor connection.';
    return;
  }
  torStarted = true;
  torCustom = custom;
  torCacheKey = `dir:${check.bridges.join(',')}`;
  app.tor_start(lines, lab?.nat || '', lab?.network || '', await torCache());
  setInterval(saveTorCache, 30 * 60 * 1000);
  renderBridges();
}

// ---- Tor bridges (Appendix F.2) ----
const savedBridges = () => (app.identity_label() ? app.section(0x05) : ramBridges);

function renderBridges(text = savedBridges()) {
  if (!TOR) return;
  const { lines, fallback } = bridges.unsaved(text);
  if (document.activeElement !== $('t-bridges') && !$('t-bridges').dataset.draft) {
    $('r-br-custom').checked = !!lines;
    $('r-br-auto').checked = !lines;
    $('t-bridges').value = lines;
    $('c-br-fallback').checked = fallback;
  }
  $('br-custom').hidden = !$('r-br-custom').checked;
  $('b-br-share').hidden = !lines;
  const using = !torStarted ? '' : torCustom ? 'Tor is using your bridges.' : "Tor is using the Tor Project's Snowflake.";
  const differs = torStarted && !!lines !== torCustom;
  const fromLink = $('t-bridges').dataset.draft === 'link' ? 'Bridges from a link: check them, then “Use this setting”.' : '';
  $('br-state').textContent = [fromLink, using, differs ? 'Your saved setting differs: it applies after a reload.' : ''].filter(Boolean).join(' ');
}

function showBridgeProblems(check) {
  const ul = $('br-problems');
  ul.replaceChildren();
  for (const p of check.problems) {
    const li = document.createElement('li');
    li.className = p.error ? 'bad' : 'dim';
    li.textContent = `Line ${p.line}: ${p.error ? 'not used' : 'note'}: ${p.text}.`;
    ul.append(li);
  }
  if (!check.usable) {
    const li = document.createElement('li');
    li.className = 'bad';
    li.textContent = 'No usable snowflake bridge: a line needs a fingerprint, url=https://… and ice=stun:….';
    ul.append(li);
  }
  ul.hidden = !ul.children.length;
}

async function applyBridges() {
  const custom = $('r-br-custom').checked;
  const text = custom ? bridges.saved($('t-bridges').value, $('c-br-fallback').checked) : '';
  if (custom) {
    const check = JSON.parse(app.bridges_check($('t-bridges').value));
    showBridgeProblems(check);
    if (!check.usable) return;
  } else $('br-problems').hidden = true;
  if (app.identity_label()) {
    if (app.set_section(0x05, text) !== 0) return;
    await persist();
  } else ramBridges = text;
  bridges.setWaiting(custom);
  delete $('t-bridges').dataset.draft;
  renderBridges(text);
  if (!torStarted) {
    startTor(custom ? bridges.effective(text) : bridges.DEFAULT_BRIDGES, custom);
  } else if (custom !== torCustom || custom) {
    const again = app.identity_label() ? 'sign in again' : 'paste your bridges again (a temporary identity keeps them only in this tab)';
    if (confirm(`Tor restarts with this setting: the page reloads, open chats end, and you ${again}. Reload now?`)) location.reload();
  }
}

// A `#b=` link fills the setting (not applied until the user says so).
function openBridgeLink(frag) {
  const lines = bridges.fromLink(frag);
  if (lines === null) return error('This bridge link is damaged.');
  openSettings('settings');
  $('r-br-custom').checked = true;
  $('br-custom').hidden = false;
  $('t-bridges').value = lines;
  $('t-bridges').dataset.draft = 'link';
  showBridgeProblems(JSON.parse(app.bridges_check(lines)));
  renderBridges();
  $('bridges-box').scrollIntoView();
}

// After a sign-in: the identity's bridges start a waiting Tor, or are offered for the next start.
function bridgesOnIdentity() {
  if (!TOR) return;
  const text = savedBridges();
  renderBridges(text);
  if (!torStarted && bridges.waiting() && text) startTor(bridges.effective(text), true);
}

// Warm start (§28.3): the public Tor directory (consensus, authority certificates,
// microdescriptors) is kept in IndexedDB between sessions. Nothing about chats, peers or keys.
let torCacheKey = '';
function torDb(mode, fn) {
  return new Promise((resolve) => {
    const req = indexedDB.open('ephem-tor', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('dir');
    req.onerror = () => resolve(undefined);
    req.onsuccess = () => {
      const db = req.result;
      const t = db.transaction('dir', mode);
      const r = fn(t.objectStore('dir'));
      t.oncomplete = () => { db.close(); resolve(r.result); };
      t.onerror = () => { db.close(); resolve(undefined); };
    };
  });
}
// A snapshot is gzip bytes; anything else (an older format) is ignored.
const torCache = () => torDb('readonly', (s) => s.get(torCacheKey)).then((v) => (v instanceof Uint8Array ? v : new Uint8Array())).catch(() => new Uint8Array());
function saveTorCache() {
  const snap = app.tor_cache();
  if (snap.length) torDb('readwrite', (s) => s.put(snap, torCacheKey)).catch(() => {});
}

// ---- boot ----------------------------------------------------------------------------------
const io = new IntersectionObserver((entries) => {
  const touched = new Set();
  for (const e of entries) {
    const c = chats.get(Number(e.target.closest('.chatroot')?.dataset.chat));
    if (!c) continue;
    const seq = Number(e.target.dataset.key.split(':')[1]);
    if (e.isIntersecting) c.visibleTheirs.add(seq);
    else c.visibleTheirs.delete(seq);
    touched.add(c);
  }
  for (const c of touched) flushRead(c);
}, { threshold: 0.6 });

async function main() {
  // A Tor invite or bridge link opens in Tor mode, every other code in direct mode (modes never
  // mix, §28.2). Channel links open where they are (channels always go through Tor).
  if (/^#[tb]=/.test(location.hash) !== TOR && /^#[iarqtb]=/.test(location.hash)) {
    location.replace((TOR ? './' : 'tor.html') + location.hash);
    return;
  }
  const frag = takeFragment();
  mod = await import(TOR ? './pkg/ephem_tor.js' : './pkg/ephem.js');
  ({ App, qr_svg_path } = mod);
  // The wasm module is fetched with the SHA-384 pinned in the page (§17.2).
  const wasmSri = document.querySelector('meta[name="ephem-wasm"]')?.content;
  const wasmUrl = new URL(TOR ? './pkg/ephem_tor_bg.wasm' : './pkg/ephem_bg.wasm', import.meta.url);
  wasm = await mod.default({ module_or_path: fetch(wasmUrl, wasmSri ? { integrity: wasmSri } : {}) });
  app = new App();
  metaPtr = app.meta_ptr();
  channels.init({ TOR, phone, app, mod: TOR ? mod : null, showPane, setTab, setStatus, error, persist, scan, download, notify, notice, ramSections: () => !app.identity_label() });
  renderIdentity();
  if (TOR) beginTor();

  for (const box of document.querySelectorAll('#v-start .codebox')) {
    box.querySelector('.copy').onclick = () => copyLink(box);
    box.querySelector('.share').onclick = () => navigator.share({ url: box.querySelector('.link').value }).catch(() => {});
  }
  for (const b of document.querySelectorAll('.tabs [role=tab]')) {
    b.onclick = () => {
      setTab(b.dataset.tab);
      if (b.dataset.tab === 'settings') return openSettings();
      // Phones: a tab opens on its list; a row (or its + button) opens a page with Back.
      if (phone()) {
        backToList();
        if (b.dataset.tab !== 'chats') channels.prepare();
        return;
      }
      if (b.dataset.tab === 'chats') return shown ? display(shown) : home();
      channels.openTab(b.dataset.tab);
    };
  }
  $('b-back').onclick = backToList;
  $('error').onclick = () => { $('error').hidden = true; };
  for (const li of document.querySelectorAll('#list-settings [data-go]')) li.onclick = () => openSettings(li.dataset.go);
  $('b-new').onclick = home;
  $('b-invite').onclick = () => {
    applyPrefs();
    app.create_invite(Number($('s-ttl').value));
    started();
  };
  $('b-code').onclick = () => ($('code-sheet').hidden ? openCodeSheet() : closeCodeSheet());
  $('b-code-close').onclick = closeCodeSheet;
  $('code-sheet').onclick = (e) => { if (e.target === $('code-sheet')) closeCodeSheet(); };
  $('b-apply').onclick = () => applyCode($('t-code').value, false);
  const notCard = () => error('That is not a contact card. Cards are links with #k=; invites go in ⌗ Code.');
  $('b-add-card').onclick = () => {
    const t = $('t-card').value.trim();
    if (app.card_nick(t) === undefined) return notCard();
    addCard(t, $('i-card-name').value.trim() || app.card_nick(t) || '');
  };
  $('t-card').oninput = () => { if (!$('i-card-name').value) $('i-card-name').value = app.card_nick($('t-card').value.trim()) || ''; };
  const cardOnly = (t) => (app.card_nick(t.trim()) !== undefined ? showAdd(t.trim()) : notCard());
  $('r-br-auto').onchange = $('r-br-custom').onchange = () => { $('br-custom').hidden = !$('r-br-custom').checked; };
  $('b-br-apply').onclick = applyBridges;
  $('t-bridges').oninput = () => { $('t-bridges').dataset.draft = '1'; };
  $('b-br-share').onclick = () => {
    $('br-link').value = bridges.shareLink($('t-bridges').value);
    $('br-link').hidden = false;
    $('br-link').select();
  };
  $('b-scan-card').onclick = () => scan(cardOnly);
  $('b-scan').onclick = () => { closeCodeSheet(); scan((t) => applyCode(t, true)); };
  $('b-scan-cancel').onclick = () => scanStop?.();
  $('b-room').onclick = () => {
    applyPrefs();
    if (app.create_room() !== 0) return;
    const c = started();
    if (!c) return;
    c.room = { owner: true, role: 0, confirmed: true };
    c.myIdx = OWNER;
    c.title = 'Your room';
    c.$('sas').hidden = true;
    c.$('peer').textContent = 'Your room';
    c.$('peer').dataset.handle = '';
    c.$('verified').textContent = 'owner';
    c.$('verified').className = 'pill ok';
    c.status('room open', 'ok');
    openChat(c, `Room created. Invite members one at a time; everyone connects ${TOR ? 'through Tor' : 'directly'} to everyone else.`);
  };
  $('b-id-receive').onclick = () => {
    if (!confirm('Receive an identity from your other device? This tab switches to it once received.')) return;
    applyPrefs();
    app.create_transfer_invite(Number($('s-ttl').value));
    const c = started();
    if (c) c.transferring = 'receiver';
  };
  $('b-backup').onclick = downloadBackup;
  $('disp').ontoggle = () => { if ($('disp').open) renderDisplay(); };
  $('b-disp-copy').onclick = () => navigator.clipboard?.writeText($('disp-text').textContent).catch(() => {});
  $('b-go-add').onclick = () => showAdd();
  $('b-share-card').onclick = () => showAdd();
  $('b-card-save').onclick = () => openSettings('identity-card');
  $('i-people').oninput = filterPeople;
  $('p-name').onchange = () => { if (person && app.rename_contact(person, $('p-name').value) === 0) persist(); };
  $('b-p-go').onclick = () => { const r = contactRows().find((x) => x.hex === person); if (r) contactAction(r); };
  $('b-p-verify').onclick = () => { if (person && app.verify_contact(person) === 0) persist(); };
  $('b-p-remove').onclick = removeContact;
  $('b-undo').onclick = undoRemoval;
  $('b-card-reset').onclick = () => {
    if (!confirm('Reset your contact card? Every card you shared stops working for a first contact.')) return;
    app.my_card(true, Number($('s-card-ttl').value));
    renderCard();
  };
  $('i-nick').onchange = () => {
    if (app.set_nick($('i-nick').value) === 0 && app.identity_label()) persist();
  };
  // Network change (§13): try an in-band ICE restart while the channel may still be up.
  // Tor: lost links we dialled are dialled again at once (a phone coming back from the
  // background, a new network), instead of waiting out the backoff.
  const netChanged = () => {
    if (TOR) app.tor_redial_now();
    else if ([...chats.values()].some((c) => c.open)) app.restart_ice();
  };
  addEventListener('online', netChanged);
  navigator.connection?.addEventListener?.('change', netChanged);
  $('b-update').onclick = applyUpdate;
  $('b-update-later').onclick = () => { $('update').hidden = true; };
  $('b-id-save').onclick = () => { $('id-save').hidden = !$('id-save').hidden; $('id-load').hidden = true; $('id-saved').hidden = true; };
  $('b-id-load').onclick = () => { $('id-load').hidden = !$('id-load').hidden; $('id-save').hidden = true; };
  $('b-id-temp').onclick = () => {
    if (app.new_temporary_identity() !== 0) return error('Close your open chats first: an identity can only change while none is open.');
    lockIdentity();
    renderIdentity();
  };
  $('b-id-do-save').onclick = saveIdentity;
  $('b-id-do-load').onclick = loadIdentity;
  for (const id of ['s-privacy', 'c-v6', 'c-read', 'c-typing']) $(id).onchange = applyPrefs;
  // Own STUN servers (§9.3; direct mode): kept in localStorage, not secret (§17.5).
  const stun = (text, save) => {
    const n = app.set_stun(text);
    $('stun-state').textContent = n === 0 ? 'Not used: only stun:host[:port] addresses, at most 4 (never turn:).'
      : text.trim() ? `New chats use ${n === 1 ? 'this STUN server' : `these ${n} STUN servers`}.` : 'New chats use the default STUN servers.';
    if (n && save) try { text.trim() ? localStorage.setItem('ephem-stun', text.trim()) : localStorage.removeItem('ephem-stun'); } catch { /* private mode */ }
  };
  try { $('t-stun').value = localStorage.getItem('ephem-stun') || ''; } catch { /* private mode */ }
  if ($('t-stun').value) stun($('t-stun').value, false);
  $('t-stun').onchange = () => stun($('t-stun').value, true);
  $('c-notify').checked = notifyOn();
  $('c-notify').onchange = async () => {
    let on = $('c-notify').checked;
    if (on && 'Notification' in globalThis) on = (await Notification.requestPermission().catch(() => 'denied')) === 'granted';
    $('c-notify').checked = on;
    try { on ? localStorage.setItem('ephem-notify', '1') : localStorage.removeItem('ephem-notify'); } catch { /* private mode */ }
  };
  document.addEventListener('visibilitychange', () => {
    flushRead(shown);
    if (TOR && !document.hidden) app.tor_redial_now();
  });
  // Keyboard: Alt+1/2/3 the tabs, Alt+↑/↓ the previous/next chat, Escape back to the list (phone).
  document.addEventListener('keydown', (e) => {
    if (e.altKey && ['1', '2', '3'].includes(e.key)) {
      e.preventDefault();
      document.querySelectorAll('.tabs [role=tab]')[Number(e.key) - 1].click();
    } else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      const list = [...document.querySelectorAll('#chats li')];
      if (!list.length) return;
      e.preventDefault();
      const i = list.findIndex((l) => l.classList.contains('active'));
      list[(i + (e.key === 'ArrowDown' ? 1 : list.length - 1)) % list.length].click();
    } else if (e.key === 'Escape' && !$('code-sheet').hidden) {
      closeCodeSheet();
    } else if (e.key === 'Escape' && !$('b-back').hidden && phone() && !e.target.closest('textarea, input')) {
      backToList();
    }
  });

  setInterval(() => {
    app.tick(document.hidden);
    const c = shown;
    if (c && !c.$('diag').hidden) c.$('diag-core').textContent = A(c).diag();
    if (TOR && !torReady) {
      const t = app.tor_status();
      if (t) $('tor-state').textContent = `Connecting to Tor through Snowflake: ${t}`;
    } else if (TOR) renderReach();
    if (c) {
      const s = c.codeExpires ? Math.max(0, Math.round((c.codeExpires - Date.now()) / 1000)) : -1;
      c.$('expiry').textContent = s >= 0 && !c.$('v-code').hidden ? `Code expires in ${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` : '';
    }
  }, 1000);
  addEventListener('pagehide', () => app.close_all());

  $('ios-note').hidden = navigator.standalone !== true;
  const build = document.querySelector('meta[name="ephem-build"]')?.content;
  if (build) $('build').textContent = `Build ${build}.`;
  registerWorker();

  home();
  // Phones start on the list (Appendix F.3.1); a link opens its pane below.
  if (phone()) backToList();
  if (TOR) setStatus('starting Tor');
  if (!frag) return;
  if (frag.startsWith('#tab=')) {
    const t = frag.slice(5);
    if (t === 'follow' || t === 'own') {
      setTab(t);
      if (phone()) channels.prepare();
      else channels.openTab(t);
    }
    return;
  }
  if (frag.startsWith('#c=')) return channels.openLink(frag);
  if (frag.startsWith('#b=')) return openBridgeLink(frag);
  if (frag.startsWith('#i=') || frag.startsWith('#t=') || frag.startsWith('#k=')) return applyCode(frag, false);
  if (await forward(frag)) {
    $('handoff-title').textContent = 'Code delivered';
    $('handoff-text').textContent = 'The code was passed to your open Ephem tab. You can close this tab.';
    showPane('v-handoff');
    return;
  }
  // An answer or reconnect code for a chat of this tab (none here: a fresh tab holds none).
  if (app.code_fits(frag)) return applyCode(frag, false);
  openCodeSheet(baseUrl() + frag);
  error('Open this code in the tab that holds the chat (or that created the invite), or paste it there.');
}

main().catch((e) => {
  setStatus('error', 'bad');
  error(`Ephem could not start: ${e?.message || e}`);
});
