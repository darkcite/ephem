// Ephem UI glue. Rust (pkg/ephem_bg.wasm) owns every piece of state; this file only renders the
// DOM and forwards input. Event contract: crates/wasm/src/lib.rs `ev` (re-entrancy rule: event
// handlers never call into `app` synchronously).
import init, { App, qr_svg_path } from './pkg/ephem.js';

const EV = { CODE: 1, CONNECTED: 2, HELLO: 3, CHAT: 4, DELIVERED: 5, DEGRADED: 6, ALIVE: 7, PEER_HIDDEN: 8, CLOSED: 9, ERROR: 10, PROGRESS: 11 };
const ST = { NONE: 0, GATHERING: 1, AWAITING: 2, CONNECTING: 3, CONNECTED: 4, CLOSED: 5 };

const MESSAGES = {
  E_INVALID_INVITE: 'This code is not a valid Ephem code. Copy the whole link again.',
  E_EXPIRED_INVITE: 'This invite has expired. Ask for a new one.',
  E_INVITE_CONSUMED: 'This invite was already answered. Each invite connects one person.',
  E_ANSWER_MISMATCH: 'This answer does not belong to the invite open in this tab. Open it in the tab that created the invite.',
  E_PROTOCOL_MISMATCH: 'Your peer uses an incompatible version of Ephem.',
  E_CRYPTO_FAILED: 'The encrypted handshake failed. The codes may have been altered.',
  E_SAS_REJECTED: 'You reported that the safety codes differ. The chat was closed: the exchange may have been intercepted.',
  E_NO_DIRECT_PATH: 'No direct path between you and your peer. Ephem never uses a relay. Common causes: a VPN such as WARP, or strict NATs on both sides. Try another network (e.g. mobile data) or LAN-only on the same Wi-Fi.',
  E_ICE_FAILED: 'The direct connection was lost.',
  E_PEER_OFFLINE: 'Your peer left the chat. Nothing was stored.',
  E_MESSAGE_TOO_LARGE: 'Message too long (max 4096 bytes).',
  E_BROWSER_UNSUPPORTED: 'This browser does not support WebRTC data channels.',
};

const $ = (id) => document.getElementById(id);
const dec = new TextDecoder();
const enc = new TextEncoder();
let wasm, app;
let myRole = null;          // 'offerer' | 'answerer'
let fromLink = false;       // at least one code arrived by link or paste → SAS prompted (§10.4)
let expiresAt = 0;
const pending = new Map();  // chat_seq → tick element

const bytes = (ptr, len) => new Uint8Array(wasm.memory.buffer, ptr, len);
const text = (ptr, len) => dec.decode(bytes(ptr, len));
const later = (fn) => queueMicrotask(fn);
const baseUrl = () => location.origin + location.pathname;

function show(view) {
  for (const v of document.querySelectorAll('.view')) v.hidden = v.id !== view;
  $('error').hidden = true;
}

function status(label, cls = '') {
  const s = $('status');
  s.textContent = label;
  s.className = 'pill status ' + cls;
}

function error(name) {
  const e = $('error');
  e.textContent = MESSAGES[name] || name;
  e.hidden = false;
}

function logLine(body, cls, seq) {
  const li = document.createElement('li');
  li.className = cls;
  li.textContent = body;
  if (cls === 'me') {
    const t = document.createElement('span');
    t.className = 'tick';
    t.textContent = '·';
    t.title = 'Sent';
    li.append(t);
    pending.set(seq, t);
  }
  const log = $('log');
  log.append(li);
  log.scrollTop = log.scrollHeight;
}

// ---- events from Rust ----------------------------------------------------------------------
globalThis.ephemEvent = (kind, num, ptr, len) => {
  switch (kind) {
    case EV.CODE: {
      const code = text(ptr, len);
      showCode(num === 1 ? 'invite' : 'answer', code);
      break;
    }
    case EV.PROGRESS:
      status(['', 'gathering', 'connecting', 'handshake'][num] || '');
      break;
    case EV.CONNECTED: {
      const b = bytes(ptr, len);
      const emoji = Array.from(b.subarray(0, 4), (x) => String.fromCodePoint(0x1f400 + x) + '️').join(' ');
      const d = String(num).padStart(6, '0');
      $('sas-digits').textContent = d.slice(0, 3) + ' ' + d.slice(3);
      $('sas-emoji').textContent = emoji;
      $('peer').textContent = dec.decode(b.subarray(4));
      $('sas').hidden = false;
      $('sas').classList.toggle('optional', !fromLink);
      $('verified').textContent = 'unverified';
      $('verified').className = 'pill';
      $('log').replaceChildren();
      pending.clear();
      logLine('Connected directly. Messages are end-to-end encrypted and exist only in these two tabs.', 'sys');
      status('connected', 'ok');
      show('v-chat');
      later(() => $('t-msg').focus());
      break;
    }
    case EV.HELLO:
      break;
    case EV.CHAT:
      logLine(text(ptr, len), 'them');
      break;
    case EV.DELIVERED:
      for (const [seq, t] of pending) {
        if (seq <= num) {
          t.textContent = '✓';
          t.title = 'Delivered';
          t.classList.add('ok');
          pending.delete(seq);
        }
      }
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
  const link = `${baseUrl()}#${kind === 'invite' ? 'i' : 'a'}=${code}`;
  $('code-title').textContent = kind === 'invite' ? 'Your invite' : 'Your answer';
  $('code-help').textContent = kind === 'invite'
    ? 'Let your peer scan this QR code, or send them the link. It works once.'
    : 'Send this answer back to the person who invited you (QR or link). The chat opens as soon as they apply it.';
  $('qr').innerHTML = qr_svg_path(link);
  $('t-link').value = link;
  $('answer-box').hidden = kind !== 'invite';
  $('b-share').hidden = !navigator.share;
  status(kind === 'invite' ? 'waiting for answer' : 'waiting for peer');
  show('v-code');
}

function ended(name) {
  pending.clear();
  const wasChat = !$('v-chat').hidden;
  $('note-title').textContent = wasChat ? 'Chat ended' : 'Could not connect';
  $('note-text').textContent = MESSAGES[name] || name;
  show('v-note');
}

function reset() {
  later(() => app.close());
  myRole = null;
  fromLink = false;
  $('t-code').value = '';
  $('t-answer').value = '';
  status('ready');
  show('v-start');
}

// ---- actions -------------------------------------------------------------------------------
const settings = () => [Number($('s-privacy').value), $('c-v6').checked];

function createInvite() {
  myRole = 'offerer';
  const [privacy, dropV6] = settings();
  const ttl = Number($('s-ttl').value);
  expiresAt = Date.now() + ttl * 1000;
  app.create_invite(privacy, dropV6, ttl);
}

function applyCode(raw, viaLink) {
  const v = raw.trim();
  if (!v) return;
  fromLink = fromLink || viaLink;
  const [privacy, dropV6] = settings();
  const isInvite = /(^|#)i=/.test(v) || (!/(^|#)a=/.test(v) && app.state() !== ST.AWAITING);
  if (isInvite) myRole = 'answerer';
  app.apply_code(v, privacy, dropV6);
}

async function copyLink() {
  const link = $('t-link').value;
  try {
    await navigator.clipboard.writeText(link);
    $('b-copy').textContent = 'Copied';
    setTimeout(() => ($('b-copy').textContent = 'Copy link'), 1500);
    // Best-effort clipboard clear after 60 s (§8.7).
    setTimeout(() => navigator.clipboard.writeText('').catch(() => {}), 60000);
  } catch {
    $('t-link').select();
  }
}

function send(ev) {
  ev.preventDefault();
  const box = $('t-msg');
  const msg = box.value;
  if (!msg.trim()) return;
  // Zero-copy on our side: UTF-8 is written straight into the wasm TX text slot (§11.6).
  const view = bytes(app.text_ptr(), app.text_cap());
  const { read, written } = enc.encodeInto(msg, view);
  if (read < msg.length) return error('E_MESSAGE_TOO_LARGE');
  const seq = app.send(written);
  if (seq < 0) return error(codeName(-seq));
  logLine(msg, 'me', seq);
  box.value = '';
  box.focus();
}

const CODE_NAMES = { 0x41: 'E_MESSAGE_TOO_LARGE', 0x35: 'E_PEER_OFFLINE', 0x40: 'E_PROTOCOL_MISMATCH', 0x21: 'E_CRYPTO_FAILED' };
const codeName = (c) => CODE_NAMES[c] || `error 0x${c.toString(16)}`;

// ---- codes arriving by link (§8.7) ---------------------------------------------------------
const bc = 'BroadcastChannel' in globalThis ? new BroadcastChannel('p2pchat-codes') : null;

function takeFragment() {
  const h = location.hash;
  if (!/^#[ia]=/.test(h)) return null;
  history.replaceState(null, '', location.pathname);
  return h;
}

function forwardAnswer(code) {
  // An answer link opened in a new tab: hand it to the tab that owns the invite.
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
    if (typeof code !== 'string' || !app || app.state() !== ST.AWAITING || !/^#a=/.test(code)) return;
    bc.postMessage({ ack: code });
    applyCode(code, true);
  });
}

// ---- boot ----------------------------------------------------------------------------------
async function main() {
  const frag = takeFragment();
  wasm = await init();
  app = new App();
  $('me').textContent = app.handle();

  $('b-invite').onclick = createInvite;
  $('b-apply').onclick = () => applyCode($('t-code').value, true);
  $('b-answer').onclick = () => applyCode($('t-answer').value, true);
  $('b-copy').onclick = copyLink;
  $('b-share').onclick = () => navigator.share({ url: $('t-link').value }).catch(() => {});
  $('b-cancel').onclick = reset;
  $('b-again').onclick = reset;
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
  $('f-send').onsubmit = send;
  $('t-msg').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) send(e);
  });

  setInterval(() => {
    app.tick(document.hidden);
    if (!$('v-code').hidden && expiresAt && myRole === 'offerer') {
      const s = Math.max(0, Math.round((expiresAt - Date.now()) / 1000));
      $('expiry').textContent = `Invite expires in ${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
    } else {
      $('expiry').textContent = '';
    }
  }, 1000);
  addEventListener('pagehide', () => app.close());

  status('ready');
  show('v-start');
  if (frag?.startsWith('#a=')) {
    if (await forwardAnswer(frag)) {
      $('note-title').textContent = 'Answer delivered';
      $('note-text').textContent = 'The answer was passed to your open Ephem tab. You can close this tab.';
      show('v-note');
      $('b-again').hidden = true;
      return;
    }
    $('t-code').value = location.origin + location.pathname + frag;
    error('Open this answer in the tab that created the invite, or paste it there.');
  } else if (frag) {
    applyCode(frag, true);
  }
}

main().catch((e) => {
  status('error', 'bad');
  error(`Ephem could not start: ${e?.message || e}`);
});
