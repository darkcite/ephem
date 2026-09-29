// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Shared helpers of the end-to-end tests (e2e_app.mjs, e2e_mvp2.mjs): a static server for the
// repository root, Chromium launch options, console/CSP watchers and PASS/FAIL bookkeeping.
import * as fs from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as url from 'node:url';
import { chromium } from 'playwright';

export const ROOT = path.resolve(path.dirname(url.fileURLToPath(import.meta.url)), '..');
export const PASS = 'correct horse battery staple';
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml',
  '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.png': 'image/png' };

/** Serves the repository root on 127.0.0.1. `rewrite(pathname, body)` may alter a text file;
 *  `route(url)` may answer a request itself (`{ type, body }`, or null to fall through). */
export function serve(rewrite = null, route = null) {
  const srv = http.createServer((q, r) => {
    const own = route?.(new URL(q.url, 'http://x'));
    if (own) { r.writeHead(own.status || 200, { 'content-type': own.type, 'cache-control': 'no-store' }); r.end(own.body); return; }
    let p = decodeURIComponent(new URL(q.url, 'http://x').pathname);
    if (p.endsWith('/')) p += 'index.html';
    const f = path.join(ROOT, path.normalize(p));
    if (!f.startsWith(ROOT + path.sep) || !fs.existsSync(f) || !fs.statSync(f).isFile()) { r.writeHead(404); r.end(); return; }
    r.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream', 'cache-control': 'no-store' });
    const alt = rewrite?.(p, () => fs.readFileSync(f, 'utf8'));
    if (alt != null) { r.end(alt); return; }
    fs.createReadStream(f).pipe(r);
  });
  return new Promise((res) => srv.listen(0, '127.0.0.1', () => res(srv)));
}

/** Chromium with raw host candidates (CI containers have no multicast DNS). */
export const launch = (args = []) => chromium.launch({
  headless: !process.env.HEADFUL,
  args: ['--disable-features=WebRtcHideLocalIpsWithMdns', ...args],
  executablePath: process.env.CHROMIUM_PATH || undefined,
  channel: process.env.E2E_BROWSER === 'chrome' && !process.env.CHROMIUM_PATH ? 'chrome' : undefined,
});

export const results = [];
export const problems = [];

export function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

/** Records page errors, console errors and CSP violations of `page`. */
export function watch(page, who) {
  page.on('pageerror', (e) => problems.push(`${who} pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error' || /Content Security Policy|Refused to/i.test(m.text())) problems.push(`${who} console: ${m.text()}`);
  });
}

/** Prints the summary and exits with 0 (all passed) or 1. */
export function finish() {
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n${results.length - failed}/${results.length} passed`);
  process.exit(failed ? 1 : 0);
}

export const msgWith = (page, cls, t) => page.locator(`#log li.${cls}`, { hasText: t }).first();
// The app's tabs (Appendix F.3): identity, contacts and connection settings live in Settings;
// invites, rooms and codes on the Chats tab's New chat pane; a chat on screen under Chats.
// Each waits until the app is up first (right after a load its controls are not wired yet).
const ready = (page) => page.waitForFunction(() => document.querySelector('#me')?.textContent !== '…', null, { timeout: 30_000 });
export const toSettings = async (page) => {
  await ready(page);
  await page.click('#tab-settings');
};
export const toChats = async (page) => {
  await ready(page);
  await page.click('#tab-chats');
};
/** Opens the "Got a code?" sheet (the header's Code button, on every tab). */
export const openCode = async (page) => {
  await ready(page);
  if (await page.isHidden('#code-sheet')) await page.click('#b-code');
};
export const toHome = async (page) => {
  await toChats(page);
  await page.click('#b-new');
};

/** Chats → ＋ New → Add a contact (paste or scan a card; share ours). */
export const toAdd = async (page) => {
  await toHome(page);
  await page.click('#b-go-add');
  await page.waitForSelector('#v-add:not([hidden])');
};
/** A contact's row in the Chats tab (docs/CONTACTS-UX.md). */
export const contactRow = async (page, name) => {
  await toChats(page);
  return page.locator('#contacts li', { hasText: name });
};

/** Max-connectivity codes (raw local IPs: CI has no mDNS), then back to New chat. */
export const openSettings = async (page) => {
  await toSettings(page);
  await page.selectOption('#s-privacy', '2');
  await toHome(page);
};

/** Invite by Alice, answer by Bob (links pasted both ways); resolves when both chats are open. */
export async function connect(a, b) {
  await openSettings(a);
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#i='), null, { timeout: 15000 });
  await openSettings(b);
  await openCode(b);
  await b.fill('#t-code', await a.inputValue('#v-code .link'));
  await b.click('#b-apply');
  await b.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#a='), null, { timeout: 15000 });
  await a.fill('#t-answer', await b.inputValue('#v-code .link'));
  await a.click('#b-answer');
}
