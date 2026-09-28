// Ephem UI glue. Rust (pkg/ephem_bg.wasm) owns every piece of protocol and chat state; this file
// renders the DOM and forwards input. Event contract: crates/wasm/src/lib.rs `ev` / `meta`.
// Re-entrancy rule: an ephemEvent handler never calls into `app` synchronously (use `later`).
import init, { App, qr_svg_path } from './pkg/ephem.js';

const EV = { CODE: 1, CONNECTED: 2, HELLO: 3, CHAT: 4, DELIVERED: 5, DEGRADED: 6, ALIVE: 7, PEER_HIDDEN: 8, CLOSED: 9, ERROR: 10,
  PROGRESS: 11, PATH: 12, SETTING: 13, EDITED: 14, DELETED: 15, EXPIRED: 16, READ: 17, TYPING: 18, SUSPENDED: 19 };
const ST = { NONE: 0, GATHERING: 1, AWAITING: 2, CONNECTING: 3, CONNECTED: 4, CLOSED: 5, SUSPENDED: 6 };
const FRAG = { 1: 'i', 2: 'a', 3: 'r', 4: 'q' };
const TTL_LABEL = { 5: '5 seconds', 30: '30 seconds', 60: '1 minute', 300: '5 minutes', 3600: '1 hour', 86400: '1 day' };
const TTL_SHORT = { 5: '5s', 30: '30s', 60: '1m', 300: '5m', 3600: '1h', 86400: '1d' };
// ErrorCode values (§19) for negative return values.
const ERR = { 0x23: 'E_DUPLICATE_SESSION', 0x01: 'E_INVALID_INVITE', 0x02: 'E_EXPIRED_INVITE', 0x03: 'E_INVITE_CONSUMED', 0x04: 'E_ANSWER_MISMATCH', 0x10: 'E_INVALID_ROOM',
  0x20: 'E_AUTH_FAILED', 0x21: 'E_CRYPTO_FAILED', 0x22: 'E_SAS_REJECTED', 0x30: 'E_ICE_FAILED', 0x31: 'E_NO_DIRECT_PATH', 0x32: 'E_RELAY_REJECTED',
  0x35: 'E_PEER_OFFLINE', 0x40: 'E_PROTOCOL_MISMATCH', 0x41: 'E_MESSAGE_TOO_LARGE', 0x42: 'E_BACKPRESSURE', 0x43: 'E_NOT_PERMITTED', 0x60: 'E_KEYFILE_INVALID' };
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
  E_RELAY_REJECTED: 'The connection went through a relay, which Ephem does not allow. The chat was closed.',
  E_PEER_OFFLINE: 'Your peer left the chat. Nothing was stored.',
  E_MESSAGE_TOO_LARGE: 'Message too long (max 4096 bytes).',
  E_BACKPRESSURE: 'Too many messages are waiting for your peer. Wait until they reconnect.',
  E_NOT_PERMITTED: 'That is not possible right now.',
  E_KEYFILE_INVALID: 'Wrong passphrase, or the key file is damaged.',
  E_BROWSER_UNSUPPORTED: 'This browser does not support WebRTC data channels.',
  E_DUPLICATE_SESSION: 'This identity is already open in another tab. Close it there first.',
};

const $ = (id) => document.getElementById(id);
const dec = new TextDecoder();
const enc = new TextEncoder();
let wasm, app;
let metaPtr = 0;               // event side-channel block (never moves: boxed at start)
let chatOpen = false;          // a chat view is live (connected at least once)
let codeExpires = 0;           // ms, for the countdown of the code on screen
let composing = null;          // { mode: 'reply' | 'edit', mine, seq }
let readSent = 0;
let typingTimer = 0;
let scanStop = null;
let pathText = '';
let lockRelease = null;        // releases the Web Lock of the saved identity in use (§7.2)
let updateWorker = null;
const msgs = new Map();        // 'm:<seq>' (mine) / 't:<seq>' (theirs) → { li, body, tick, text, meta }
const visibleTheirs = new Set();

const later = (fn) => queueMicrotask(fn);
const mem = (ptr, len) => new Uint8Array(wasm.memory.buffer, ptr, len);
const text = (ptr, len) => dec.decode(mem(ptr, len));
const baseUrl = () => location.origin + location.pathname;
const keyOf = (mine, seq) => `${mine ? 'm' : 't'}:${seq}`;
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

function quoteText(mine, seq) {
  const m = msgs.get(keyOf(mine, seq));
  if (!m || m.deleted) return 'Message unavailable';
  return (mine ? 'You: ' : 'Peer: ') + m.text.slice(0, 80);
}

function addMessage(mine, seq, body, ttl, reply) {
  const li = document.createElement('li');
  li.className = mine ? 'me' : 'them';
  li.dataset.key = keyOf(mine, seq);
  if (reply) {
    const q = document.createElement('span');
    q.className = 'quote';
    q.dataset.ref = keyOf(reply.mine, reply.seq);
    q.textContent = quoteText(reply.mine, reply.seq);
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
  const m = { li, body: b, tick, meta, text: body, mine, seq, deleted: false, level: 0 };
  msgs.set(li.dataset.key, m);
  const follow = mine || atBottom();
  $('log').append(li);
  if (follow) toBottom();
  if (!mine) io.observe(li);
  return m;
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
  const [mine, seq] = [key[0] === 'm', Number(key.slice(2))];
  for (const q of document.querySelectorAll(`.quote[data-ref="${key}"]`)) q.textContent = quoteText(mine, seq);
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
  add('Reply', () => startComposing('reply', m));
  if (m.mine) add('Edit', () => startComposing('edit', m));
  add(m.mine ? 'Delete for everyone' : 'Delete for me', () => deleteMessage(m));
  add('Copy', () => navigator.clipboard?.writeText(m.text).catch(() => {}));
  m.li.append(acts);
}

function startComposing(mode, m) {
  composing = { mode, mine: m.mine, seq: m.seq };
  $('composing-text').textContent = mode === 'edit' ? 'Editing your message' : 'Reply to ' + quoteText(m.mine, m.seq);
  $('composing').hidden = false;
  if (mode === 'edit') $('t-msg').value = m.text;
  $('t-msg').focus();
}

function stopComposing() {
  composing = null;
  $('composing').hidden = true;
}

function deleteMessage(m) {
  const r = app.delete(m.mine, m.seq);
  if (r < 0) return error(errName(r));
  if (m.mine) markDeleted(m.li.dataset.key, 'You deleted this message');
  else removeMessage(m.li.dataset.key);
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
globalThis.ephemEvent = (kind, num, ptr, len) => {
  switch (kind) {
    case EV.CODE: {
      const code = text(ptr, len);
      codeExpires = num === 1 || num === 3 ? Date.now() + Number($('s-ttl').value) * 1000 : 0;
      if (num <= 2) showCode(num, code);
      else showResumeCode(num, code);
      later(renderExposure);
      break;
    }
    case EV.PROGRESS:
      status(['', 'gathering', 'connecting', 'handshake'][num] || '');
      break;
    case EV.CONNECTED: {
      const b = mem(ptr, len);
      const resumed = new DataView(wasm.memory.buffer, metaPtr, 16).getUint8(0) === 1;
      status('connected', 'ok');
      codeExpires = 0;
      if (resumed) {
        $('resume').hidden = true;
        sysLine('Reconnected directly. Pending messages are being delivered.');
        break;
      }
      const d = String(num).padStart(6, '0');
      $('sas-digits').textContent = d.slice(0, 3) + ' ' + d.slice(3);
      $('sas-emoji').textContent = Array.from(b.subarray(0, 4), (x) => String.fromCodePoint(0x1f400 + x) + '️').join(' ');
      $('peer').textContent = dec.decode(b.subarray(4));
      $('sas').hidden = false;
      $('sas').classList.remove('optional');
      $('verified').textContent = 'unverified';
      $('verified').className = 'pill';
      $('log').replaceChildren();
      msgs.clear();
      visibleTheirs.clear();
      readSent = 0;
      stopComposing();
      $('resume').hidden = true;
      $('diag').hidden = true;
      $('s-chat-ttl').value = '0';
      chatOpen = true;
      sysLine('Connected directly. Messages are end-to-end encrypted and exist only in these two tabs.');
      show('v-chat');
      later(() => $('t-msg').focus());
      break;
    }
    case EV.HELLO:
      // Both codes scanned in person: the SAS is shown but not prompted (§10.4).
      if (num === 1 && $('verified').textContent !== 'verified') {
        $('sas').classList.add('optional');
        $('verified').textContent = 'met in person';
      }
      break;
    case EV.CHAT: {
      const meta = new DataView(wasm.memory.buffer, metaPtr, 16);
      const ttl = meta.getUint32(0, true);
      const reply = meta.getUint8(4) ? { mine: meta.getUint8(5) === 1, seq: meta.getFloat64(8, true) } : null;
      addMessage(false, num, text(ptr, len), ttl, reply);
      $('peer-state').textContent = '';
      break;
    }
    case EV.DELIVERED:
      setTick(num, 1);
      break;
    case EV.READ:
      setTick(num, 2);
      break;
    case EV.SETTING:
      $('s-chat-ttl').value = String(num);
      sysLine(num ? `Your peer set messages to disappear after ${TTL_LABEL[num]}.` : 'Your peer turned off disappearing messages.');
      break;
    case EV.EDITED: {
      const m = msgs.get(keyOf(false, num));
      if (m && !m.deleted) {
        m.text = text(ptr, len);
        m.body.textContent = m.text;
        m.meta.textContent = 'edited';
        refreshQuotes(m.li.dataset.key);
      }
      break;
    }
    case EV.DELETED:
      markDeleted(keyOf(num > 0, Math.abs(num)), 'Message deleted');
      break;
    case EV.EXPIRED:
      removeMessage(keyOf(num > 0, Math.abs(num)));
      break;
    case EV.TYPING:
      $('peer-state').textContent = num ? 'typing…' : '';
      break;
    case EV.DEGRADED:
      status('no response', 'bad');
      $('peer-state').textContent = 'connection problem…';
      break;
    case EV.ALIVE:
      status('connected', 'ok');
      $('peer-state').textContent = '';
      break;
    case EV.PEER_HIDDEN:
      $('peer-state').textContent = num ? 'in background' : '';
      break;
    case EV.PATH:
      pathText = text(ptr, len);
      renderPath();
      break;
    case EV.SUSPENDED:
      status('disconnected', 'bad');
      $('peer-state').textContent = '';
      if (chatOpen) {
        $('resume').hidden = false;
        $('resume').querySelector('.codebox').hidden = true;
        sysLine('Direct path lost. Share a reconnect code to continue.');
      }
      break;
    case EV.CLOSED: {
      const name = text(ptr, len);
      status('closed', 'bad');
      later(() => ended(name));
      break;
    }
    case EV.ERROR:
      error(text(ptr, len));
      break;
  }
};

// ---- views ---------------------------------------------------------------------------------
function showCode(kind, code) {
  $('code-title').textContent = kind === 1 ? 'Your invite' : 'Your answer';
  $('code-help').textContent = kind === 1
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

function ended(name) {
  const wasChat = chatOpen;
  chatOpen = false;
  msgs.clear();
  visibleTheirs.clear();
  $('log').replaceChildren();
  $('note-title').textContent = wasChat ? 'Chat ended' : 'Could not connect';
  $('note-text').textContent = MESSAGES[name] || name;
  $('b-again').hidden = false;
  show('v-note');
}

function reset() {
  later(() => app.close());
  chatOpen = false;
  codeExpires = 0;
  $('t-code').value = '';
  $('t-answer').value = '';
  $('exposure').hidden = true;
  status('ready');
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
}

// ---- actions -------------------------------------------------------------------------------
function applyPrefs() {
  app.set_prefs(Number($('s-privacy').value), $('c-v6').checked, $('c-read').checked, $('c-typing').checked);
}

function applyCode(raw, scanned) {
  const v = raw.trim();
  if (!v) return;
  applyPrefs();
  app.apply_code(v, scanned);
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
    const m = msgs.get(keyOf(true, composing.seq));
    if (m) {
      m.text = msg;
      m.body.textContent = msg;
      m.meta.textContent = 'edited';
      refreshQuotes(m.li.dataset.key);
    }
  } else {
    const reply = composing?.mode === 'reply' ? composing : null;
    const seq = app.send(n, reply?.mine ?? false, reply?.seq ?? 0);
    if (seq < 0) return error(errName(seq));
    addMessage(true, seq, msg, app.chat_ttl(), reply && { mine: reply.mine, seq: reply.seq });
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
  download(blob, `ephem-${label.replace(/[^\w-]+/g, '_') || 'identity'}.p2pkey`);
  $('t-keytext').value = b64u(blob);
  $('id-saved').hidden = false;
  renderIdentity();
  lockIdentity();
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
  if (app.load_identity(bytes, pw) === 0) {
    if (!(await lockIdentity())) {
      app.new_temporary_identity();
      renderIdentity();
      return error('E_DUPLICATE_SESSION');
    }
    $('id-load').hidden = true;
    $('i-file').value = '';
    $('t-keyin').value = '';
    renderIdentity();
  }
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
  if (!/^#[iarq]=/.test(h)) return null;
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

// ---- boot ----------------------------------------------------------------------------------
const io = new IntersectionObserver((entries) => {
  for (const e of entries) {
    const seq = Number(e.target.dataset.key.slice(2));
    if (e.isIntersecting) visibleTheirs.add(seq);
    else visibleTheirs.delete(seq);
  }
  flushRead();
}, { threshold: 0.6 });

async function main() {
  const frag = takeFragment();
  // The wasm module is fetched with the SHA-384 pinned in index.html (§17.2).
  const wasmSri = document.querySelector('meta[name="ephem-wasm"]')?.content;
  wasm = await init({ module_or_path: fetch(new URL('./pkg/ephem_bg.wasm', import.meta.url), wasmSri ? { integrity: wasmSri } : {}) });
  app = new App();
  metaPtr = app.meta_ptr();
  renderIdentity();

  const codeBoxes = document.querySelectorAll('.codebox');
  for (const box of codeBoxes) {
    box.querySelector('.copy').onclick = () => copyLink(box);
    box.querySelector('.share').onclick = () => navigator.share({ url: box.querySelector('.link').value }).catch(() => {});
  }
  $('b-invite').onclick = () => { applyPrefs(); app.create_invite(Number($('s-ttl').value)); };
  $('b-apply').onclick = () => applyCode($('t-code').value, false);
  $('b-scan').onclick = () => scan((t) => applyCode(t, true));
  $('b-answer').onclick = () => applyCode($('t-answer').value, false);
  $('b-scan-answer').onclick = () => scan((t) => applyCode(t, true));
  $('b-cancel').onclick = reset;
  $('b-again').onclick = reset;
  $('b-scan-cancel').onclick = () => scanStop?.();
  $('b-leave').onclick = () => {
    later(() => app.close());
    ended('E_PEER_OFFLINE');
    $('note-text').textContent = 'You left the chat. Nothing was stored.';
  };
  $('b-sas-ok').onclick = () => {
    $('sas').hidden = true;
    $('verified').textContent = 'verified';
    $('verified').className = 'pill ok';
  };
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
    const s = codeExpires ? Math.max(0, Math.round((codeExpires - Date.now()) / 1000)) : -1;
    $('expiry').textContent = s >= 0 && !$('v-code').hidden ? `Code expires in ${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` : '';
  }, 1000);
  addEventListener('pagehide', () => app.close());

  $('ios-note').hidden = navigator.standalone !== true;
  const build = document.querySelector('meta[name="ephem-build"]')?.content;
  if (build) $('build').textContent = `Build ${build}.`;
  registerWorker();

  status('ready');
  show('v-start');
  if (!frag) return;
  if (frag.startsWith('#i=')) return applyCode(frag, false);
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
