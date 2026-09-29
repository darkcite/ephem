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
export const openSettings = async (page) => {
  await page.$eval('#v-start details', (d) => { d.open = true; });
  await page.selectOption('#s-privacy', '2');
};

/** Invite by Alice, answer by Bob (links pasted both ways); resolves when both chats are open. */
export async function connect(a, b) {
  await openSettings(a);
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#i='), null, { timeout: 15000 });
  await openSettings(b);
  await b.fill('#t-code', await a.inputValue('#v-code .link'));
  await b.click('#b-apply');
  await b.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#a='), null, { timeout: 15000 });
  await a.fill('#t-answer', await b.inputValue('#v-code .link'));
  await a.click('#b-answer');
}
