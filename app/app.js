// Ephem UI glue. Rust (pkg/ephem_bg.wasm) owns every piece of protocol and chat state; this file
// renders the DOM and forwards input. Event contract: crates/wasm/src/lib.rs `ev` / `meta`.
// Re-entrancy rule: an ephemEvent handler never calls into `app` synchronously (use `later`).
// Tor mode (§28): tor.html (html data-mode="tor") loads the Tor build, pkg/ephem_tor*, instead;
// the direct page never downloads it.
import * as slots from './slots.js';

const TOR = document.documentElement.dataset.mode === 'tor';
// Tor mode: the Snowflake rendezvous and bridge built into the app (as in Tor Browser, §28.3).
// Its STUN servers serve only the Snowflake proxy connections, never a chat peer (§28.5).
const SNOWFLAKE = {
  // The direct broker, then its CDN URL (reachable where the broker's name is blocked; no
  // domain fronting, which browsers cannot do). Both are in tor.html's CSP.
  broker: 'https://snowflake-broker.torproject.net/,https://1098762253.rsc.cdn77.org/',
  // Both Snowflake bridges of the Tor Project (snowflake-01, snowflake-02), as in Tor Browser:
  // one failing does not stop Tor.
  fingerprint: '2B280B23E1107BB62ABFC40DDCC8824814F80A72,8838024498816A039FCBBAB14E6F40A0843051FA',
  ice: 'stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478,stun:stun.sonetel.com:3478,stun:stun.uls.co.za:3478,stun:stun.voipgate.com:3478,stun:stun.voys.nl:3478',
  nat: '',
  network: '', // empty: the real Tor network
};
let App, qr_svg_path;

const EV = { CODE: 1, CONNECTED: 2, HELLO: 3, CHAT: 4, DELIVERED: 5, DEGRADED: 6, ALIVE: 7, PEER_HIDDEN: 8, CLOSED: 9, ERROR: 10,
  PROGRESS: 11, PATH: 12, SETTING: 13, EDITED: 14, DELETED: 15, EXPIRED: 16, READ: 17, TYPING: 18, SUSPENDED: 19,
  REACTION: 20, PEER_READY: 21, IDENTITY_SENT: 22, IDENTITY_RECEIVED: 23, ROOM: 24, ROOM_CLOSED: 25, TOR: 26, CARD: 27 };
// Meta block offsets (crates/wasm/src/lib.rs `meta`).
const META = { TTL: 0, HAS_REPLY: 4, SENDER: 5, REPLY_SEQ: 8, RESUMED: 0, MEMBER: 16, LEN: 24 };
const PENDING = 0xff;           // member index of a joiner the owner has not admitted yet
const OWNER = 0;
const ROLE = ['owner', 'member', 'observer'];
const REACTIONS = ['👍', '❤️', '😂', '😮', '😢', '🙏'];
const FLAG_GROUP = 2;
const FLAG_TRANSFER = 4;
const FLAG_OBSERVER = 8;
const CONTACT_HAS_ONION = 2; // contact flags (crates/crypto/src/contacts.rs `cflags`)
const ST = { NONE: 0, GATHERING: 1, AWAITING: 2, CONNECTING: 3, CONNECTED: 4, CLOSED: 5, SUSPENDED: 6 };
const FRAG = { 1: 'i', 2: 'a', 3: 'r', 4: 'q', 5: 't', 6: 'k' };
const TTL_LABEL = { 5: '5 seconds', 30: '30 seconds', 60: '1 minute', 300: '5 minutes', 3600: '1 hour', 86400: '1 day' };
const TTL_SHORT = { 5: '5s', 30: '30s', 60: '1m', 300: '5m', 3600: '1h', 86400: '1d' };
// ErrorCode values (§19) for negative return values.
const ERR = { 0x11: 'E_ROOM_FULL', 0x12: 'E_ROOM_DISPOSED', 0x13: 'E_NOT_OWNER', 0x23: 'E_DUPLICATE_SESSION', 0x24: 'E_NOT_A_CONTACT', 0x01: 'E_INVALID_INVITE', 0x02: 'E_EXPIRED_INVITE', 0x03: 'E_INVITE_CONSUMED', 0x04: 'E_ANSWER_MISMATCH', 0x10: 'E_INVALID_ROOM',
  0x20: 'E_AUTH_FAILED', 0x21: 'E_CRYPTO_FAILED', 0x22: 'E_SAS_REJECTED', 0x30: 'E_ICE_FAILED', 0x31: 'E_NO_DIRECT_PATH', 0x32: 'E_RELAY_REJECTED',
  0x35: 'E_PEER_OFFLINE', 0x36: 'E_TOR_UNAVAILABLE', 0x40: 'E_PROTOCOL_MISMATCH', 0x41: 'E_MESSAGE_TOO_LARGE', 0x42: 'E_BACKPRESSURE', 0x43: 'E_NOT_PERMITTED', 0x60: 'E_KEYFILE_INVALID' };
const MESSAGES = {
  E_INVALID_INVITE: 'This is not a valid Ephem code. Copy the whole link again.',
  E_EXPIRED_INVITE: 'This code has expired. Ask for a new one.',
  E_INVITE_CONSUMED: 'This invite was already answered. Each invite connects one person.',
  E_ANSWER_MISMATCH: 'This answer does not belong to the code open in this tab. Open it in the tab that created the code.',
  E_INVALID_ROOM: 'This reconnect code belongs to a different chat.',
  E_AUTH_FAILED: 'This reconnect code was made by someone else, not by your peer.',
  E_PROTOCOL_MISMATCH: 'Your peer uses an incompatible version of Ephem.',
  E_CRYPTO_FAILED: 'The encrypted handshake failed. The codes may have been altered.',
  E_SAS_REJECTED: 'You reported that the safety codes differ. The chat was closed: the exchange may have been intercepted.',
  E_NO_DIRECT_PATH: 'No direct path between you and your peer. Ephem never uses a relay. Common causes: a VPN such as WARP, or strict NATs on both sides. Try another network (e.g. mobile data), or LAN only on the same Wi-Fi.',
  E_ICE_FAILED: 'The direct connection was lost.',
  E_TOR_UNAVAILABLE: 'Tor is not reachable from here (the Snowflake broker may be blocked on this network). Tor mode never falls back to a direct connection.',
  E_RELAY_REJECTED: 'The connection went through a relay, which Ephem does not allow. The chat was closed.',
  E_PEER_OFFLINE: 'Your peer left the chat. Nothing was stored.',
  E_MESSAGE_TOO_LARGE: 'Message too long (max 4096 bytes).',
  E_BACKPRESSURE: 'Too many messages are waiting for your peer. Wait until they reconnect.',
  E_NOT_PERMITTED: 'That is not possible right now.',
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
let chatOpen = false;          // a chat view is live (connected at least once)
let codeExpires = 0;           // ms, for the countdown of the code on screen
let composing = null;          // { mode: 'reply' | 'edit', sender, seq }
let readSent = 0;
let typingTimer = 0;
let scanStop = null;
let pathText = '';
let torReady = false;
let cardRequest = false;       // the open chat came through our contact card: ask first (§28.4)
let lockRelease = null;        // releases the Web Lock of the saved identity in use (§7.2)
let updateWorker = null;
let transferring = null;       // identity transfer (§7.6): 'receiver' (new device) | 'sender' (old device)
let xferDone = false;
let receivedBlob = null;       // the received key file, still passphrase-encrypted
let peerNick = '';             // what the peer calls itself (HELLO); never authentication
let myIdx = 0;                 // our member index: messages are identified by (sender, seq)
let room = null;               // { owner, role, confirmed } while in a room (§14)
const names = new Map();       // room member index → display name
const deliveredBy = new Map(); // room member index → cumulative delivered seq of our messages
const removedByMe = new Set(); // owner: members just removed (no "no longer in the room" line)
const msgs = new Map();        // '<sender>:<seq>' → { li, body, tick, text, meta, sender, seq, mine }
const visibleTheirs = new Set();

const later = (fn) => queueMicrotask(fn);
const mem = (ptr, len) => new Uint8Array(wasm.memory.buffer, ptr, len);
const text = (ptr, len) => dec.decode(mem(ptr, len));
const baseUrl = () => location.origin + location.pathname;
const keyOf = (sender, seq) => `${sender}:${seq}`;
const metaView = () => new DataView(wasm.memory.buffer, metaPtr, META.LEN);
const member = () => metaView().getUint8(META.MEMBER);
const nameOf = (idx) => (idx === myIdx ? 'You' : names.get(idx) || (room ? `member ${idx}` : 'Peer'));
const errName = (neg) => ERR[-neg] || `error 0x${(-neg).toString(16)}`;
const b64u = (u8) => btoa(String.fromCharCode(...u8)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const unb64u = (s) => {
  s = s.trim().replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(s + '==='.slice((s.length + 3) % 4)), (c) => c.charCodeAt(0));
};

// ---- small view helpers --------------------------------------------------------------------
function show(view) {
  for (const v of document.querySelectorAll('.view')) v.hidden = v.id !== view;
  $('error').hidden = true;
}

function status(label, cls = '') {
  $('status').textContent = label;
  $('status').className = 'pill status ' + cls;
}

function error(name) {
  $('error').textContent = MESSAGES[name] || name;
  $('error').hidden = false;
}

function renderCodeBox(box, kind, code) {
  const link = `${baseUrl()}#${FRAG[kind]}=${code}`;
  box.querySelector('.qr').innerHTML = qr_svg_path(link);
  box.querySelector('.link').value = link;
  box.querySelector('.share').hidden = !navigator.share;
  box.hidden = false;
}

function renderExposure() {
  const lines = [...new Set(app.exposure().trim().split('\n').filter(Boolean))]; // one IP may appear with several ports
  const pretty = lines.map((l) => (l.startsWith('v6') ? 'IPv6 ' : 'IPv4 ') + l.slice(3));
  const box = $('exposure');
  box.querySelector('.addrs').textContent = pretty.length ? pretty.join('\n') : 'No public address in this code (LAN only, or STUN was unreachable).';
  box.querySelector('.v6warn').hidden = !(lines.some((l) => l.startsWith('v4')) && lines.some((l) => l.startsWith('v6')));
  box.hidden = false;
  $('diag-exposure').textContent = pretty.length ? pretty.join(', ') : 'no public address';
}

// ---- messages ------------------------------------------------------------------------------
// The page scrolls (composer is sticky): follow new messages only if the reader is at the bottom.
const atBottom = () => innerHeight + scrollY >= document.documentElement.scrollHeight - 160;
const toBottom = () => scrollTo(0, document.documentElement.scrollHeight);

function sysLine(t) {
  const follow = atBottom();
  const li = document.createElement('li');
  li.className = 'sys';
  li.textContent = t;
  $('log').append(li);
  if (follow) toBottom();
}

function quoteText(sender, seq) {
  const m = msgs.get(keyOf(sender, seq));
  if (!m || m.deleted) return 'Message unavailable';
  return `${nameOf(sender)}: ${m.text.slice(0, 80)}`;
}

function addMessage(sender, seq, body, ttl, reply) {
  const mine = sender === myIdx;
  const li = document.createElement('li');
  li.className = mine ? 'me' : 'them';
  li.dataset.key = keyOf(sender, seq);
  if (room && !mine) {
    const who = document.createElement('span');
    who.className = 'who';
    who.textContent = nameOf(sender);
    li.append(who);
  }
  if (reply) {
    const q = document.createElement('span');
    q.className = 'quote';
    q.dataset.ref = keyOf(reply.sender, reply.seq);
    q.textContent = quoteText(reply.sender, reply.seq);
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
  msgs.set(li.dataset.key, m);
  const follow = mine || atBottom();
  $('log').append(li);
  if (follow) toBottom();
  if (!mine && !room) io.observe(li);
  if (mine && room) roomTick(m);
  return m;
}

// Room delivery: "✓ k/N", N = the other members now in the room (§14.3).
function roomTick(m) {
  const others = [...names.keys()].filter((i) => i !== myIdx);
  const k = others.filter((i) => (deliveredBy.get(i) || 0) >= m.seq).length;
  m.tick.textContent = others.length ? `✓ ${k}/${others.length}` : '🕓';
  m.tick.title = others.length ? `Delivered to ${k} of ${others.length}` : 'Nobody else in the room yet';
  m.tick.classList.toggle('ok', others.length > 0 && k === others.length);
}

function setTick(upto, level) {
  for (const m of msgs.values()) {
    if (!m.mine || m.seq > upto || m.level >= level || m.deleted) continue;
    m.level = level;
    m.tick.textContent = level === 1 ? '✓' : '✓✓';
    m.tick.title = level === 1 ? 'Delivered' : 'Read';
    m.tick.classList.add('ok');
  }
}

// Re-renders every quote of message `key` (after an edit, a delete or an expiry).
function refreshQuotes(key) {
  const [sender, seq] = key.split(':').map(Number);
  for (const q of document.querySelectorAll(`.quote[data-ref="${key}"]`)) q.textContent = quoteText(sender, seq);
}

function markDeleted(key, label) {
  const m = msgs.get(key);
  if (!m) return;
  m.deleted = true;
  m.text = '';
  m.li.classList.add('deleted');
  m.body.textContent = label;
  m.meta.textContent = '';
  m.li.querySelector('.acts')?.remove();
  refreshQuotes(key);
}

function removeMessage(key) {
  const m = msgs.get(key);
  if (!m) return;
  io.unobserve(m.li);
  m.li.remove();
  msgs.delete(key);
  refreshQuotes(key);
}

function toggleActions(m) {
  const old = m.li.querySelector('.acts');
  for (const a of document.querySelectorAll('.log .acts')) a.remove();
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
  const observer = room?.role === 2;
  if (!observer) {
    add('Reply', () => startComposing('reply', m));
    add('React', () => showPicker(m));
  }
  if (m.mine) add('Edit', () => startComposing('edit', m));
  if (room?.owner && !m.mine) add('Delete for everyone', () => deleteMessage(m, true));
  add(m.mine ? 'Delete for everyone' : 'Delete for me', () => deleteMessage(m, false));
  add('Copy', () => navigator.clipboard?.writeText(m.text).catch(() => {}));
  m.li.append(acts);
}

// One reaction per person per message; the latest wins, empty removes (§11.7).
function renderReactions(m) {
  m.reacts.replaceChildren();
  for (const [by, e] of m.reactions) {
    if (!e) continue;
    const t = document.createElement('span');
    t.textContent = `${e} ${by === myIdx ? 'you' : room ? nameOf(by) : 'peer'}`;
    m.reacts.append(t);
  }
}

function showPicker(m) {
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
      const r = app.react(m.sender, m.seq, writeText(emoji));
      if (r < 0) return error(errName(r));
      m.reactions.set(myIdx, emoji);
      renderReactions(m);
    };
    picker.append(b);
  }
  m.li.append(picker);
}

// "Anything that renders as more than one grapheme is rejected" (§11.7).
const oneGrapheme = (s) => !s || !globalThis.Intl?.Segmenter || [...new Intl.Segmenter().segment(s)].length === 1;

function startComposing(mode, m) {
  composing = { mode, sender: m.sender, seq: m.seq };
  $('composing-text').textContent = mode === 'edit' ? 'Editing your message' : 'Reply to ' + quoteText(m.sender, m.seq);
  $('composing').hidden = false;
  if (mode === 'edit') $('t-msg').value = m.text;
  $('t-msg').focus();
}

function stopComposing() {
  composing = null;
  $('composing').hidden = true;
}

// Ours: for everyone. Someone else's: for me only, or for everyone by the room owner.
function deleteMessage(m, moderate) {
  if (!m.mine && !moderate) {
    if (!room?.owner) app.delete(m.sender, m.seq); // local only: drops its self-destruct timer (an owner's call would moderate)
    return removeMessage(m.li.dataset.key);
  }
  const r = app.delete(m.sender, m.seq);
  if (r < 0) return error(errName(r));
  markDeleted(m.li.dataset.key, m.mine ? 'You deleted this message' : 'You removed this message');
}

// Read receipts: a message counts as read when it is on screen and the page is visible (§11.7).
function flushRead() {
  if (document.hidden || !visibleTheirs.size) return;
  const max = Math.max(...visibleTheirs);
  if (max > readSent) {
    readSent = max;
    app.mark_read(max);
  }
}

// ---- events from Rust ----------------------------------------------------------------------
// Every event names the link it came from (meta MEMBER: the peer's member index). In a 1:1 chat
// there is one link; in a room the member's link to the owner plays that part (the "primary").
const primary = (idx) => !room || (!room.owner && idx === OWNER);

globalThis.ephemEvent = (kind, num, ptr, len) => {
  const from = member();
  switch (kind) {
    case EV.CODE: {
      const code = text(ptr, len);
      if (room?.owner && (num === 1 || num === 5)) {
        showRoomInvite(num, code);
        break;
      }
      codeExpires = num === 1 || num === 3 || num === 5 ? Date.now() + Number($('s-ttl').value) * 1000 : 0;
      if (num <= 2 || num === 5) showCode(num, code);
      else showResumeCode(num, code);
      later(renderExposure);
      break;
    }
    case EV.PROGRESS:
      if (primary(from)) status(['', 'gathering', 'connecting', 'handshake'][num] || '');
      break;
    case EV.CONNECTED: {
      const b = mem(ptr, len);
      const resumed = metaView().getUint8(META.RESUMED) === 1;
      if (!primary(from)) {
        if (room.owner && from === PENDING) {
          hideRoomInvite();
          sysLine('Someone answered your invite. Admitting them to the room…');
        }
        later(renderRoom);
        break;
      }
      status('connected', 'ok');
      codeExpires = 0;
      if (resumed) {
        $('resume').hidden = true;
        sysLine(`Reconnected ${TOR ? 'through Tor' : 'directly'}. Pending messages are being delivered.`);
        break;
      }
      const d = String(num).padStart(6, '0');
      const digits = d.slice(0, 3) + ' ' + d.slice(3);
      const emoji = Array.from(b.subarray(0, 4), (x) => String.fromCodePoint(0x1f400 + x) + '️').join(' ');
      const handle = dec.decode(b.subarray(4));
      peerNick = '';
      if (transferring) {
        later(() => showTransfer(digits, emoji));
        break;
      }
      if (!room) myIdx = from === 0 ? 1 : 0; // 1:1: the offerer is 0, the answerer 1
      $('sas-digits').textContent = digits;
      $('sas-emoji').textContent = emoji;
      $('sas-help').textContent = room
        ? 'Compare it with the room owner on another channel, for example a call. It proves the room (and its member list) comes from them.'
        : 'Compare it with your peer on another channel, for example a call. If it differs, someone may be in the middle.';
      $('peer').textContent = handle;
      $('peer').dataset.handle = handle;
      $('imp-warn').hidden = true;
      $('sas').hidden = false;
      $('sas').classList.remove('optional');
      $('verified').textContent = 'unverified';
      $('verified').className = 'pill';
      if (cardRequest) later(showCardRequest);
      openChat(room ? 'Connected to the room owner. The member list arrives next.'
        : TOR ? 'Connected through Tor: neither of you sees the other\'s IP address. Messages are end-to-end encrypted and exist only in these two tabs.'
          : 'Connected directly. Messages are end-to-end encrypted and exist only in these two tabs.');
      break;
    }
    case EV.HELLO: {
      if (transferring) break;
      if (!primary(from)) {
        later(renderRoom);
        break;
      }
      peerNick = text(ptr, len);
      if (cardRequest) later(showCardRequest);
      // Both codes scanned in person: the SAS is shown but not prompted (§10.4).
      if (num === 1 && $('verified').textContent === 'unverified') {
        $('sas').classList.add('optional');
        $('verified').textContent = 'met in person';
      }
      later(renderPeer);
      if (room) later(renderRoom);
      break;
    }
    case EV.REACTION: {
      const mv = metaView();
      const m = msgs.get(keyOf(mv.getUint8(META.SENDER), num));
      const e = text(ptr, len);
      if (m && !m.deleted && oneGrapheme(e)) {
        m.reactions.set(from, e);
        renderReactions(m);
      }
      break;
    }
    case EV.PEER_READY:
      $('xfer-state').textContent = 'The new device confirmed the code.';
      break;
    case EV.IDENTITY_SENT:
      xferDone = true;
      $('xfer-state').textContent = '';
      $('xfer-sent').hidden = false;
      later(() => app.close());
      break;
    case EV.IDENTITY_RECEIVED:
      xferDone = true;
      receivedBlob = mem(ptr, len).slice(); // documented copy: the key file leaves wasm to be kept by JS until unlocked
      $('xfer-state').textContent = '';
      $('xfer-unlock').hidden = false;
      later(() => { app.close(); $('i-xfer-pass').focus(); });
      break;
    case EV.CHAT: {
      const mv = metaView();
      const ttl = mv.getUint32(META.TTL, true);
      const reply = mv.getUint8(META.HAS_REPLY) ? { sender: mv.getUint8(META.SENDER), seq: mv.getFloat64(META.REPLY_SEQ, true) } : null;
      addMessage(from, num, text(ptr, len), ttl, reply);
      if (!room) $('peer-state').textContent = '';
      break;
    }
    case EV.DELIVERED:
      if (!room) {
        setTick(num, 1);
        break;
      }
      deliveredBy.set(from, Math.max(num, deliveredBy.get(from) || 0));
      for (const m of msgs.values()) if (m.mine && !m.deleted && m.seq <= num) roomTick(m);
      break;
    case EV.READ:
      setTick(num, 2);
      break;
    case EV.SETTING:
      $('s-chat-ttl').value = String(num);
      if (room) sysLine(num ? `The owner set messages to disappear after ${TTL_LABEL[num]}.` : 'The owner turned off disappearing messages.');
      else sysLine(num ? `Your peer set messages to disappear after ${TTL_LABEL[num]}.` : 'Your peer turned off disappearing messages.');
      break;
    case EV.EDITED: {
      const m = msgs.get(keyOf(from, num));
      if (m && !m.deleted) {
        m.text = text(ptr, len);
        m.body.textContent = m.text;
        m.meta.textContent = 'edited';
        refreshQuotes(m.li.dataset.key);
      }
      break;
    }
    case EV.DELETED: {
      const sender = metaView().getUint8(META.SENDER);
      markDeleted(keyOf(sender, num), sender !== from && from === OWNER ? 'Removed by the room owner' : 'Message deleted');
      break;
    }
    case EV.EXPIRED:
      removeMessage(keyOf(metaView().getUint8(META.SENDER), num));
      break;
    case EV.TYPING:
      $('peer-state').textContent = num ? 'typing…' : '';
      break;
    case EV.DEGRADED:
      if (!primary(from)) break;
      status('no response', 'bad');
      $('peer-state').textContent = room ? 'owner not responding…' : 'connection problem…';
      break;
    case EV.ALIVE:
      if (!primary(from)) break;
      status('connected', 'ok');
      $('peer-state').textContent = '';
      break;
    case EV.PEER_HIDDEN:
      if (!room) $('peer-state').textContent = num ? 'in background' : '';
      break;
    case EV.PATH:
      if (!primary(from)) break;
      pathText = text(ptr, len);
      renderPath();
      break;
    case EV.SUSPENDED:
      if (!primary(from)) {
        later(renderRoom);
        break;
      }
      status('disconnected', 'bad');
      $('peer-state').textContent = room ? 'owner unreachable' : '';
      if (TOR) {
        // No reconnect codes over Tor: whoever dialled dials the onion again (§28.5).
        if (chatOpen) sysLine('The Tor connection dropped. Reconnecting through Tor…');
      } else if (chatOpen) {
        $('resume').hidden = false;
        $('resume').querySelector('.codebox').hidden = true;
        sysLine(room ? 'Direct path to the room owner lost. Share a reconnect code with the owner to continue.' : 'Direct path lost. Share a reconnect code to continue.');
      }
      break;
    case EV.CLOSED: {
      const name = text(ptr, len);
      if (room && !primary(from)) {
        later(renderRoom);
        break;
      }
      status('closed', 'bad');
      if (!xferDone) later(() => ended(room ? ROOM_MESSAGES[name] || name : name));
      break;
    }
    case EV.ROOM:
      later(renderRoom);
      break;
    case EV.ROOM_CLOSED: {
      const name = text(ptr, len);
      if (room) later(() => ended(ROOM_MESSAGES[name] || name));
      break;
    }
    case EV.ERROR:
      error(text(ptr, len));
      break;
    case EV.CARD:
      if (num === 1) {
        cardRequest = true;
        later(showCardRequest);
      } else later(persist);
      break;
    case EV.TOR: {
      const t = text(ptr, len);
      if (num === 2) {
        torReady = true;
        later(saveTorCache);
        $('tor-state').textContent = `Reachable through Tor while this tab is open (${t.slice(0, 8)}….onion): by your invites, and by your contacts when you are signed in.`;
        if (!$('v-start').hidden) status('Tor ready', 'ok');
      } else if (num === 3) {
        $('tor-state').textContent = `Tor failed: ${t}`;
        status('Tor failed', 'bad');
        error('E_TOR_UNAVAILABLE');
      }
      break;
    }
  }
};

// ---- views ---------------------------------------------------------------------------------
function showCode(kind, code) {
  $('code-title').textContent = transferring === 'receiver' ? 'Receive an identity' : kind === 1 || kind === 5 ? 'Your invite' : 'Your answer';
  $('code-help').textContent = transferring === 'receiver'
    ? 'On the old device, sign in with the identity you want to move, then scan this code (or open the link) and send back the answer.'
    : transferring === 'sender'
      ? 'Show this answer to the new device (QR or link). Both devices then show a safety code to compare.'
      : kind === 5
        ? 'Let your peer scan this QR code, or send them the link (they open it in Tor mode). It works once, and no answer is needed: keep this tab open until they connect through Tor.'
        : kind === 1
        ? 'Let your peer scan this QR code, or send them the link. It works once.'
        : 'Send this answer back to the person who invited you (QR or link). The chat opens as soon as they apply it.';
  renderCodeBox(document.querySelector('#v-code .codebox'), kind, code);
  $('answer-box').hidden = kind !== 1;
  status(kind === 1 ? 'waiting for answer' : 'waiting for peer');
  show('v-code');
}

function showResumeCode(kind, code) {
  const box = $('resume').querySelector('.codebox');
  box.querySelector('.resume-help').textContent = kind === 3
    ? 'Send this reconnect code to your peer. Then scan or paste their answer below.'
    : 'Send this answer back to your peer. The chat reconnects as soon as they apply it.';
  renderCodeBox(box, kind, code);
  $('resume').hidden = false;
  status(kind === 3 ? 'waiting for answer' : 'waiting for peer');
}

// The chat view, fresh: a 1:1 chat or a room (member: once its owner link is up).
function openChat(line) {
  $('log').replaceChildren();
  msgs.clear();
  visibleTheirs.clear();
  deliveredBy.clear();
  readSent = 0;
  stopComposing();
  $('resume').hidden = true;
  $('diag').hidden = true;
  $('s-chat-ttl').value = '0';
  // Rooms: only the owner sets the timer; observers only read (§11.7).
  $('s-chat-ttl').disabled = !!room && !room.owner;
  $('f-send').hidden = room?.role === 2;
  $('room').hidden = !room;
  chatOpen = true;
  sysLine(line);
  show('v-chat');
  later(() => { renderPeer(); if (room) renderRoom(); $('t-msg').focus(); });
}

// A used or withdrawn invite must never be picked up again.
function hideRoomInvite() {
  const box = $('room-invite');
  box.hidden = true;
  box.querySelector('.link').value = '';
  box.querySelector('.qr').replaceChildren();
}

function showRoomInvite(kind, code) {
  renderCodeBox($('room-invite').querySelector('.codebox'), kind, code);
  $('t-room-answer').value = '';
  $('room-invite').hidden = false;
}

// Members, their roles and our direct link to each (§14); the owner can remove members.
function renderRoom() {
  if (!room) return;
  const [me, role, , version, confirmed] = app.room_info().split('\t').map(Number);
  if (Number.isNaN(me)) return;
  myIdx = me;
  room.role = role;
  room.confirmed = confirmed === 1;
  const rows = app.room_members().split('\n').filter(Boolean).map((l) => l.split('\t'));
  const before = new Map(names);
  names.clear();
  for (const [idx, , handle, , nick] of rows) names.set(Number(idx), nick ? `${nick} (${handle})` : handle);
  // Departures, whatever the path (left, removed, link lost for good), from the signed state.
  for (const [idx, name] of before) if (!names.has(idx) && !removedByMe.delete(idx)) sysLine(`${name} is no longer in the room.`);
  const ul = $('members');
  ul.replaceChildren();
  const LINK = { me: 'you', connected: TOR ? 'via Tor' : 'direct', connecting: 'connecting…', suspended: 'reconnecting…', 'no-path': 'no direct path', none: 'not connected' };
  for (const [idx, r, , link, , sas] of rows) {
    const i = Number(idx);
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'grow';
    name.innerHTML = '<b></b> <span class="role"></span> <span></span> <span class="dim"></span>';
    name.querySelector('b').textContent = i === myIdx ? names.get(i) + ' (you)' : names.get(i);
    name.querySelector('.role').textContent = ROLE[Number(r)];
    const st = name.children[2];
    st.textContent = link === 'me' ? '' : LINK[link] || link;
    st.className = link === 'connected' ? 'link-ok' : link === 'no-path' || link === 'suspended' ? 'link-bad' : 'dim';
    // The owner compares each member's safety code with that member (§10.4).
    if (room.owner && i !== myIdx && sas !== '0') name.querySelector('.dim').textContent = `SAS ${sas.padStart(6, '0').replace(/(\d{3})(\d{3})/, '$1 $2')}`;
    li.append(name);
    if (room.owner && i !== OWNER) {
      const rm = document.createElement('button');
      rm.textContent = 'Remove';
      rm.onclick = () => {
        if (!confirm(`Remove ${names.get(i)} from the room?`)) return;
        removedByMe.add(i);
        if (app.room_remove(i) === 0) sysLine(`You removed ${names.get(i)}.`);
        else removedByMe.delete(i);
      };
      li.append(rm);
    }
    ul.append(li);
  }
  $('room-count').textContent = `${rows.length} / 16`;
  $('room-role').textContent = ROLE[role] || '';
  $('room-owner').hidden = !room.owner;
  $('f-send').hidden = role === 2;
  const others = rows.length - 2;
  // Joined as the only member: the invite prompt already said every future member sees our IP.
  if (!room.owner && !room.confirmed && version > 0 && others <= 0) {
    room.confirmed = true;
    app.room_connect();
  }
  $('room-confirm').hidden = room.owner || room.confirmed || version === 0 || others <= 0;
  $('room-confirm-text').textContent = `Connect directly to ${others} other member${others === 1 ? '' : 's'}? Each of them will see your IP address, and you theirs (Ephem never uses a relay).`;
  for (const m of msgs.values()) if (m.mine && !m.deleted) roomTick(m);
}

// Contact name, verification and the impersonation warning in the chat header (§7.5).
function renderPeer() {
  if (room) {
    $('b-save-contact').hidden = true;
    $('imp-warn').hidden = true;
    if (!room.owner) $('peer').textContent = `Room of ${$('peer').dataset.handle || 'the owner'}${peerNick ? ` “${peerNick}”` : ''}`;
    return;
  }
  const [flags, nick] = (app.peer_contact() || '').split('\t');
  const handle = $('peer').dataset.handle || '';
  const contact = flags !== undefined && flags !== '';
  const verified = contact && (Number(flags) & 1) === 1;
  $('peer').textContent = contact ? `${nick || handle}${verified ? ' ✔' : ''}` : peerNick ? `${handle} “${peerNick}”` : handle;
  $('peer').title = contact ? `Contact ${handle}` : peerNick ? 'The name in quotes is chosen by the peer, not verified' : '';
  $('b-save-contact').hidden = !app.identity_label() || contact;
  if (verified) {
    // The key is already pinned by a verified contact: no SAS prompt (§10.4).
    $('sas').hidden = true;
    $('verified').textContent = 'verified contact';
    $('verified').className = 'pill ok';
  }
  const imp = peerNick ? app.impersonates(peerNick) : '';
  $('imp-warn').hidden = !imp;
  $('imp-warn').textContent = imp ? `This is not the “${imp}” you verified: the name matches but the key is different. Compare the safety code.` : '';
}

function showTransfer(digits, emoji) {
  $('xfer-digits').textContent = digits;
  $('xfer-emoji').textContent = emoji;
  $('xfer-title').textContent = transferring === 'receiver' ? 'Receive an identity' : `Send identity “${app.identity_label()}”`;
  $('xfer-help').textContent = transferring === 'receiver'
    ? 'Compare the safety code with the old device. After both confirm, the old device sends its encrypted key file.'
    : 'Compare the safety code with the new device. Only confirm if both show the same code and the other device is yours.';
  $('xfer-sas-actions').hidden = false;
  $('xfer-state').textContent = '';
  $('xfer-unlock').hidden = true;
  $('xfer-sent').hidden = true;
  status('connected', 'ok');
  show('v-transfer');
}

function endTransfer() {
  transferring = null;
  xferDone = false;
  receivedBlob = null;
  $('i-xfer-pass').value = '';
  reset();
}

function ended(name) {
  if (transferring) transferring = null;
  cardRequest = false;
  $('card-req').hidden = true;
  $('log').hidden = false;
  const wasChat = chatOpen;
  const wasRoom = !!room;
  chatOpen = false;
  room = null;
  names.clear();
  msgs.clear();
  visibleTheirs.clear();
  $('log').replaceChildren();
  $('note-title').textContent = wasChat ? (wasRoom ? 'Room closed' : 'Chat ended') : 'Could not connect';
  $('note-text').textContent = MESSAGES[name] || name;
  $('b-again').textContent = 'Start over';
  $('b-again').hidden = false;
  show('v-note');
}

function reset() {
  later(() => app.close());
  chatOpen = false;
  room = null;
  names.clear();
  removedByMe.clear();
  for (const id of ['room', 'room-invite', 'room-confirm', 'card-req']) $(id).hidden = true;
  cardRequest = false;
  $('log').hidden = false;
  $('s-chat-ttl').disabled = false;
  $('f-send').hidden = false;
  codeExpires = 0;
  $('t-code').value = '';
  $('t-answer').value = '';
  $('exposure').hidden = true;
  // Old codes are useless (single use) and must never be picked up again.
  for (const box of document.querySelectorAll('.codebox')) {
    box.querySelector('.link').value = '';
    box.querySelector('.qr').replaceChildren();
  }
  status(TOR && !torReady ? 'starting Tor' : 'ready');
  renderIdentity();
  show('v-start');
}

// The UI never shows raw IP addresses unless "show addresses" is on (§18).
function renderPath() {
  $('diag-path').textContent = $('c-addr').checked
    ? pathText
    : pathText.replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, '•••').replace(/\[[0-9a-fA-F:.]+\]/g, '[•••]');
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
  $('b-card').hidden = !label;
  if (!label) $('card').hidden = true;
  if (document.activeElement !== $('i-nick')) $('i-nick').value = app.nick();
  renderContacts();
  renderSlots();
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

function renderContacts() {
  const saved = !!app.identity_label();
  $('contacts-card').hidden = !saved;
  if (!saved) return;
  const rows = app.contacts().split('\n').filter(Boolean).map((l) => l.split('\t'));
  $('contacts-count').textContent = `${rows.length} / 256`;
  const ul = $('contacts');
  ul.replaceChildren();
  for (const [hex, flags, nick, handle] of rows) {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'grow';
    name.innerHTML = '<b></b> <span class="ok"></span> <span class="dim"></span>';
    name.querySelector('b').textContent = nick || handle;
    name.querySelector('.ok').textContent = Number(flags) & 1 ? '✔' : '';
    name.querySelector('.dim').textContent = handle;
    const rename = document.createElement('button');
    rename.textContent = 'Rename';
    rename.onclick = () => {
      const n = prompt('Name for this contact (only you see it)', nick);
      if (n !== null && app.rename_contact(hex, n) === 0) persist();
    };
    const del = document.createElement('button');
    del.textContent = 'Remove';
    del.onclick = () => {
      if (confirm(`Remove ${nick || handle} from your contacts?`) && app.remove_contact(hex) === 0) persist();
    };
    li.append(name);
    // Tor mode: a contact with an onion key is dialled directly, no code (§28.7).
    if (TOR && Number(flags) & CONTACT_HAS_ONION) {
      const call = document.createElement('button');
      call.textContent = 'Connect';
      call.className = 'primary';
      call.onclick = () => connectContact(hex, nick || handle);
      li.append(call);
    }
    li.append(rename, del);
    ul.append(li);
  }
  if (!rows.length) ul.innerHTML = '<li class="dim">No contacts yet. Add one from their contact card below, or after a chat use “＋ contact”.</li>';
}

// After a change of contacts or nickname: re-encrypt (the file key stays in wasm memory, §7.3),
// update the remembered slot, and flag the downloaded backup as out of date.
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
  if (app.load_identity(blob, pw) !== 0) return false;
  if (!(await lockIdentity())) {
    app.new_temporary_identity();
    renderIdentity();
    error('E_DUPLICATE_SESSION');
    return false;
  }
  if (rememberIt) await remember(blob);
  renderIdentity();
  return true;
}

// ---- actions -------------------------------------------------------------------------------
function applyPrefs() {
  app.set_prefs(Number($('s-privacy').value), $('c-v6').checked, $('c-read').checked, $('c-typing').checked);
}

function applyCode(raw, scanned) {
  const v = raw.trim();
  if (!v) return;
  if (app.card_nick(v) !== undefined) return addCard(v);
  const info = app.code_info(v);
  if ((info & 0xff) === 1 && (info >> 8) & FLAG_TRANSFER) {
    // Someone asks for this identity (§7.6).
    if (!app.identity_label()) return error('Sign in with the identity you want to move first, then open this code again.');
    if (!confirm(`This code asks for your identity “${app.identity_label()}”. Only continue if the other device is yours. Continue?`)) return;
    transferring = 'sender';
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
  if (app.apply_code(v, scanned) !== 0) {
    if (transferring === 'sender') transferring = null;
    return;
  }
  if (kind === 1 || kind === 5) {
    room = group ? { owner: false, role: (info >> 8) & FLAG_OBSERVER ? 2 : 1, confirmed: kind === 5 } : null;
    myIdx = 1;
  }
  if (kind === 5) {
    status('connecting via Tor');
    $('note-title').textContent = 'Connecting through Tor…';
    $('note-text').textContent = 'Reaching your peer\'s onion service. This usually takes 10–60 seconds; their tab must be open.';
    $('b-again').textContent = 'Cancel';
    show('v-note');
  }
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

function send(ev) {
  ev.preventDefault();
  const box = $('t-msg');
  const msg = box.value;
  if (!msg.trim()) return;
  const n = writeText(msg);
  if (n < 0) return error('E_MESSAGE_TOO_LARGE');
  if (composing?.mode === 'edit') {
    const r = app.edit(composing.seq, n);
    if (r < 0) return error(errName(r));
    const m = msgs.get(keyOf(myIdx, composing.seq));
    if (m) {
      m.text = msg;
      m.body.textContent = msg;
      m.meta.textContent = 'edited';
      refreshQuotes(m.li.dataset.key);
    }
  } else {
    const reply = composing?.mode === 'reply' ? composing : null;
    const seq = app.send(n, reply?.sender ?? 0, reply?.seq ?? 0);
    if (seq < 0) return error(errName(seq));
    addMessage(myIdx, seq, msg, app.chat_ttl(), reply && { sender: reply.sender, seq: reply.seq });
  }
  stopComposing();
  box.value = '';
  box.focus();
  clearTimeout(typingTimer);
}

function onTyping() {
  if (!chatOpen) return;
  app.typing(true);
  clearTimeout(typingTimer);
  typingTimer = setTimeout(() => app.typing(false), 4000);
}

function setChatTtl() {
  const v = Number($('s-chat-ttl').value);
  const r = app.set_ttl(v);
  if (r < 0) {
    $('s-chat-ttl').value = String(app.chat_ttl());
    return error(errName(r));
  }
  sysLine(v ? `You set messages to disappear after ${TTL_LABEL[v]}.` : 'You turned off disappearing messages.');
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
  const blob = app.save_identity(label, pw); // pw is wiped by Rust
  if (!blob.length) return;
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

async function unlockTransferred() {
  const pw = enc.encode($('i-xfer-pass').value);
  $('i-xfer-pass').value = '';
  if (!receivedBlob || !(await signIn(receivedBlob, pw, $('c-xfer-remember').checked))) return;
  if (confirm('Signed in. Download a backup of the key file now?')) download(receivedBlob, keyFileName());
  endTransfer();
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
    if (found && /#[iarq]=/.test(found)) {
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
  if (!/^#[iarqtk]=/.test(h)) return null;
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
  if (chatOpen && !confirm('Updating reloads Ephem and ends the current chat. Update now?')) return;
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

function addCard(text) {
  if (!app.identity_label()) return error('Sign in with a saved identity first: contacts live in its key file.');
  const suggested = app.card_nick(text) || '';
  const name = prompt(`Add ${suggested ? `“${suggested}”` : 'the owner of this card'} as a contact? Name (only you see it):`, suggested);
  if (name === null) return;
  if (app.add_card(text, name) !== 0) return;
  persist();
  $('t-code').value = '';
  $('t-card').value = '';
  status(TOR ? 'contact added: Connect to chat' : 'contact added');
  show('v-start');
}

// The chat came through our card from someone who is not a contact yet (§28.4 case 3).
function showCardRequest() {
  if (!cardRequest || !chatOpen) return;
  $('card-req-text').textContent = `${peerNick || $('peer').dataset.handle || 'Someone'} (from your contact card) wants to connect. Compare the safety code, then accept or decline.`;
  $('card-req').hidden = false;
  // Nothing of the chat is shown before the user accepts.
  $('f-send').hidden = true;
  $('log').hidden = true;
}

function answerCardRequest(accept) {
  cardRequest = false;
  $('card-req').hidden = true;
  if (!accept) return $('b-leave').click();
  $('f-send').hidden = false;
  $('log').hidden = false;
  if (app.save_contact(peerNick || $('peer').dataset.handle || '') === 0) persist().then(renderPeer);
}

// ---- Tor mode (§28) --------------------------------------------------------------------------
function connectContact(hex, name) {
  if (app.contact_connect(hex) !== 0) return;
  room = null;
  myIdx = 1;
  status('connecting via Tor');
  $('note-title').textContent = `Connecting to ${name} through Tor…`;
  $('note-text').textContent = 'Their Ephem must be open in Tor mode, signed in, with no other chat. This usually takes 10–60 seconds.';
  $('b-again').textContent = 'Cancel';
  show('v-note');
}

// Test hooks, set before the page loads (a page script cannot set them: the CSP allows only our
// files): `ephemTorLab`, the offline lab's broker, bridge and Tor network (checks/tor-lab);
// `ephemTorLog`, an arti log level for the console (diagnostics of live runs).
async function startTor() {
  const c = globalThis.ephemTorLab || SNOWFLAKE;
  const log = globalThis.ephemTorLab?.log || globalThis.ephemTorLog;
  if (log) app.tor_log(log);
  torCacheKey = `dir:${c.fingerprint}`;
  app.tor_start(c.broker, c.fingerprint, c.ice, c.nat, c.network, await torCache());
  setInterval(saveTorCache, 30 * 60 * 1000);
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
  for (const e of entries) {
    const seq = Number(e.target.dataset.key.split(':')[1]);
    if (e.isIntersecting) visibleTheirs.add(seq);
    else visibleTheirs.delete(seq);
  }
  flushRead();
}, { threshold: 0.6 });

async function main() {
  // A Tor invite opens in Tor mode, every other code in direct mode (modes never mix, §28.2).
  if (location.hash.startsWith('#t=') !== TOR && /^#[iarqt]=/.test(location.hash)) {
    location.replace((TOR ? './' : 'tor.html') + location.hash);
    return;
  }
  const frag = takeFragment();
  const mod = await import(TOR ? './pkg/ephem_tor.js' : './pkg/ephem.js');
  ({ App, qr_svg_path } = mod);
  // The wasm module is fetched with the SHA-384 pinned in the page (§17.2).
  const wasmSri = document.querySelector('meta[name="ephem-wasm"]')?.content;
  const wasmUrl = new URL(TOR ? './pkg/ephem_tor_bg.wasm' : './pkg/ephem_bg.wasm', import.meta.url);
  wasm = await mod.default({ module_or_path: fetch(wasmUrl, wasmSri ? { integrity: wasmSri } : {}) });
  app = new App();
  metaPtr = app.meta_ptr();
  renderIdentity();
  if (TOR) startTor();

  const codeBoxes = document.querySelectorAll('.codebox');
  for (const box of codeBoxes) {
    box.querySelector('.copy').onclick = () => copyLink(box);
    box.querySelector('.share').onclick = () => navigator.share({ url: box.querySelector('.link').value }).catch(() => {});
  }
  $('b-invite').onclick = () => { applyPrefs(); app.create_invite(Number($('s-ttl').value)); };
  $('b-apply').onclick = () => applyCode($('t-code').value, false);
  const cardOnly = (t) => (app.card_nick(t.trim()) !== undefined ? addCard(t.trim()) : error('That is not a contact card. Cards are links with #k=; invites go in “Got a code?”.'));
  $('b-add-card').onclick = () => cardOnly($('t-card').value);
  $('b-scan-card').onclick = () => scan(cardOnly);
  $('b-scan').onclick = () => scan((t) => applyCode(t, true));
  $('b-answer').onclick = () => applyCode($('t-answer').value, false);
  $('b-scan-answer').onclick = () => scan((t) => applyCode(t, true));
  $('b-cancel').onclick = () => { transferring = null; reset(); };
  $('b-again').onclick = reset;
  $('b-scan-cancel').onclick = () => scanStop?.();
  $('b-leave').onclick = () => {
    if (room?.owner && names.size > 1 && !confirm('Close the room for everyone?')) return;
    const note = room ? (room.owner ? 'You closed the room. Nothing was stored.' : 'You left the room. Nothing was stored.') : 'You left the chat. Nothing was stored.';
    later(() => app.close());
    ended('E_PEER_OFFLINE');
    $('note-text').textContent = note;
  };
  $('b-room').onclick = () => {
    applyPrefs();
    room = { owner: true, role: 0, confirmed: true };
    myIdx = OWNER;
    app.create_room();
    $('sas').hidden = true;
    $('peer').textContent = 'Your room';
    $('peer').dataset.handle = '';
    $('verified').textContent = 'owner';
    $('verified').className = 'pill ok';
    status('room open', 'ok');
    openChat(`Room created. Invite members one at a time; everyone connects ${TOR ? 'through Tor' : 'directly'} to everyone else.`);
  };
  $('b-room-invite').onclick = () => { hideRoomInvite(); applyPrefs(); app.room_invite(false, Number($('s-ttl').value)); };
  $('b-room-observer').onclick = () => { hideRoomInvite(); applyPrefs(); app.room_invite(true, Number($('s-ttl').value)); };
  $('b-room-answer').onclick = () => applyCode($('t-room-answer').value, false);
  $('b-scan-room').onclick = () => scan((t) => applyCode(t, true));
  $('b-room-connect').onclick = () => { app.room_connect(); renderRoom(); };
  $('b-room-decline').onclick = () => $('b-leave').click();
  $('b-sas-ok').onclick = () => {
    $('sas').hidden = true;
    $('verified').textContent = 'verified';
    $('verified').className = 'pill ok';
    app.confirm_sas();
    if (app.peer_contact()) persist().then(renderPeer);
  };
  $('b-save-contact').onclick = () => {
    const n = prompt('Save as contact. Name (only you see it):', peerNick || $('peer').dataset.handle || '');
    if (n !== null && app.save_contact(n) === 0) persist().then(renderPeer);
  };
  $('b-xfer-ok').onclick = () => {
    if (app.confirm_sas() !== 0) return;
    $('xfer-sas-actions').hidden = true;
    $('xfer-state').textContent = transferring === 'receiver' ? 'Waiting for the identity…' : 'Waiting for the new device to confirm…';
  };
  $('b-xfer-bad').onclick = () => {
    later(() => app.close());
    ended('E_SAS_REJECTED');
  };
  $('b-xfer-unlock').onclick = unlockTransferred;
  $('b-xfer-keep').onclick = endTransfer;
  $('b-xfer-remove').onclick = async () => {
    if (!confirm('Remove this identity from this device? Make sure the other device unlocked it.')) return;
    await slots.remove(app.lock_name());
    app.new_temporary_identity();
    lockIdentity();
    endTransfer();
  };
  $('b-id-receive').onclick = () => {
    if (!confirm('Receive an identity from your other device? This tab switches to it once received.')) return;
    transferring = 'receiver';
    applyPrefs();
    app.create_transfer_invite(Number($('s-ttl').value));
  };
  $('b-backup').onclick = downloadBackup;
  $('b-card').onclick = () => { $('card').hidden = !$('card').hidden; if (!$('card').hidden) renderCard(); };
  $('b-card-reset').onclick = () => {
    if (!confirm('Reset your contact card? Every card you shared stops working for a first contact.')) return;
    app.my_card(true, Number($('s-card-ttl').value));
    renderCard();
  };
  $('b-card-accept').onclick = () => answerCardRequest(true);
  $('b-card-decline').onclick = () => answerCardRequest(false);
  $('i-nick').onchange = () => {
    if (app.set_nick($('i-nick').value) === 0 && app.identity_label()) persist();
  };
  $('b-restart').onclick = () => app.restart_ice();
  // Network change (§13): try an in-band ICE restart while the channel may still be up.
  const netChanged = () => { if (chatOpen) app.restart_ice(); };
  addEventListener('online', netChanged);
  navigator.connection?.addEventListener?.('change', netChanged);
  $('b-sas-bad').onclick = () => {
    later(() => app.close());
    ended('E_SAS_REJECTED');
  };
  $('b-info').onclick = () => { $('diag').hidden = !$('diag').hidden; if (!$('diag').hidden) renderExposure(); };
  $('b-update').onclick = applyUpdate;
  $('b-update-later').onclick = () => { $('update').hidden = true; };
  $('b-drop').onclick = () => app.drop_path();
  $('c-addr').onchange = renderPath;
  $('b-resume').onclick = () => app.create_resume(Number($('s-ttl').value));
  $('b-scan-resume').onclick = () => scan((t) => applyCode(t, true));
  $('b-resume-apply').onclick = () => { applyCode($('t-resume').value, false); $('t-resume').value = ''; };
  $('b-composing-x').onclick = () => { if (composing?.mode === 'edit') $('t-msg').value = ''; stopComposing(); };
  $('s-chat-ttl').onchange = setChatTtl;
  $('log').onclick = (e) => {
    const li = e.target.closest('li[data-key]');
    const m = li && msgs.get(li.dataset.key);
    if (m) toggleActions(m);
  };
  for (const b of document.querySelectorAll('.drop-v6')) {
    b.onclick = () => { $('c-v6').checked = true; applyPrefs(); b.closest('.v6warn').textContent = 'IPv6 will be left out of your next codes.'; };
  }
  $('b-id-save').onclick = () => { $('id-save').hidden = !$('id-save').hidden; $('id-load').hidden = true; $('id-saved').hidden = true; };
  $('b-id-load').onclick = () => { $('id-load').hidden = !$('id-load').hidden; $('id-save').hidden = true; };
  $('b-id-temp').onclick = () => {
    if (app.new_temporary_identity() !== 0) return error('E_NOT_PERMITTED');
    lockIdentity();
    renderIdentity();
  };
  $('b-id-do-save').onclick = saveIdentity;
  $('b-id-do-load').onclick = loadIdentity;
  for (const id of ['s-privacy', 'c-v6', 'c-read', 'c-typing']) $(id).onchange = applyPrefs;
  $('f-send').onsubmit = send;
  $('t-msg').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) send(e);
    else if (e.key === 'Escape') stopComposing();
  });
  $('t-msg').addEventListener('input', onTyping);
  document.addEventListener('visibilitychange', flushRead);

  setInterval(() => {
    app.tick(document.hidden);
    if (!$('diag').hidden) $('diag-core').textContent = app.diag();
    if (TOR && !torReady && !$('v-start').hidden) {
      const t = app.tor_status();
      if (t) $('tor-state').textContent = `Connecting to Tor through Snowflake: ${t}`;
    }
    const s = codeExpires ? Math.max(0, Math.round((codeExpires - Date.now()) / 1000)) : -1;
    $('expiry').textContent = s >= 0 && !$('v-code').hidden ? `Code expires in ${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` : '';
  }, 1000);
  addEventListener('pagehide', () => app.close());

  $('ios-note').hidden = navigator.standalone !== true;
  const build = document.querySelector('meta[name="ephem-build"]')?.content;
  if (build) $('build').textContent = `Build ${build}.`;
  registerWorker();

  status(TOR ? 'starting Tor' : 'ready');
  show('v-start');
  if (!frag) return;
  if (frag.startsWith('#i=') || frag.startsWith('#t=') || frag.startsWith('#k=')) return applyCode(frag, false);
  if (await forward(frag)) {
    $('note-title').textContent = 'Code delivered';
    $('note-text').textContent = 'The code was passed to your open Ephem tab. You can close this tab.';
    $('b-again').hidden = true;
    show('v-note');
    return;
  }
  $('t-code').value = baseUrl() + frag;
  error('Open this code in the tab that holds the chat (or that created the invite), or paste it there.');
}

main().catch((e) => {
  status('error', 'bad');
  error(`Ephem could not start: ${e?.message || e}`);
});
