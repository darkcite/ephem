// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Tor bridges (docs/P2P-CHAT.md Appendix F.2): which Snowflake setup the Tor client starts with.
// Rust parses and checks the lines (`App.bridges_check`, crates/tor/src/bridge.rs); this file
// only picks the lines, keeps the per-browser "wait for my bridges" flag and makes share links.

// The built-in setup, as bridge lines: the Tor Project's two Snowflake bridges (snowflake-01,
// -02, as in Tor Browser), the direct broker and its CDN URL (reachable where the broker's name
// is blocked; no domain fronting, which browsers cannot do), and STUN servers for the proxy
// connections only, never for a chat peer (§28.5).
const STUN = 'stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478,stun:stun.sonetel.com:3478,stun:stun.uls.co.za:3478,stun:stun.voipgate.com:3478,stun:stun.voys.nl:3478';
export const DEFAULT_BRIDGES = [
  `snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://snowflake-broker.torproject.net/ ice=${STUN}`,
  `snowflake 192.0.2.4:80 8838024498816A039FCBBAB14E6F40A0843051FA url=https://1098762253.rsc.cdn77.org/ ice=${STUN}`,
].join('\n');

// A comment line in the saved text (the parser skips comments): also use the defaults.
const FALLBACK = '# ephem: also use the Tor Project Snowflake';
// Set in this browser when the user's bridges are in use: Tor then waits for them (from the key
// file at sign-in, or pasted) instead of starting with the defaults. Says only "custom bridges".
const WAIT = 'ephem-bridges';

export const waiting = () => { try { return localStorage.getItem(WAIT) === '1'; } catch { return false; } };
export function setWaiting(on) {
  try { on ? localStorage.setItem(WAIT, '1') : localStorage.removeItem(WAIT); } catch { /* private mode */ }
}

/** The saved form: the user's lines, and the fallback marker when asked for. */
export const saved = (lines, fallback) => (lines.trim() ? (fallback ? `${FALLBACK}\n` : '') + lines.trim() : '');
/** `{lines, fallback}` of the saved form. */
export function unsaved(text) {
  const fallback = text.startsWith(FALLBACK);
  return { lines: fallback ? text.slice(FALLBACK.length).trim() : text.trim(), fallback };
}
/** The lines Tor starts with. */
export const effective = (text) => {
  const { lines, fallback } = unsaved(text);
  return fallback ? `${lines}\n${DEFAULT_BRIDGES}` : lines;
};

// Share links: `#b=` + base64url of the lines (UTF-8). Opening one fills the setting; it is
// applied only when the user says so.
export function shareLink(lines) {
  const b = new TextEncoder().encode(lines.trim());
  let s = '';
  for (let i = 0; i < b.length; i++) s += String.fromCharCode(b[i]);
  return new URL(`#b=${btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')}`, location.href.split('#')[0]).href;
}
export function fromLink(text) {
  const m = /#b=([A-Za-z0-9_-]+)/.exec(text);
  if (!m) return null;
  try {
    const s = atob(m[1].replace(/-/g, '+').replace(/_/g, '/'));
    return new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(s, (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}
