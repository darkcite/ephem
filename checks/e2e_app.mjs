// End-to-end test of the Ephem app (/app/) in real Chromium: two isolated browser contexts
// (Alice, Bob) run the whole MVP-1 flow over a real WebRTC DataChannel.
//
//   invite → Bob pastes the link → answer → answer link opened in a NEW tab of Alice's browser
//   (BroadcastChannel hand-off, §8.7) → Noise KK → equal SAS → chat both ways → ✓ → leave.
//
// Usage: node checks/e2e_app.mjs            (headless; build first with ./build.sh)
//        HEADFUL=1 node checks/e2e_app.mjs
//        E2E_BROWSER=chrome node checks/e2e_app.mjs   (your installed Google Chrome, no download)
// Exit code 0 = PASS. Also fails on any CSP violation or page error.
import * as fs from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as url from 'node:url';
import { chromium } from 'playwright';

const ROOT = path.resolve(path.dirname(url.fileURLToPath(import.meta.url)), '..');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.png': 'image/png' };

function serve() {
  const srv = http.createServer((q, r) => {
    let p = decodeURIComponent(new URL(q.url, 'http://x').pathname);
    if (p.endsWith('/')) p += 'index.html';
    const f = path.join(ROOT, path.normalize(p));
    if (!f.startsWith(ROOT + path.sep) || !fs.existsSync(f) || !fs.statSync(f).isFile()) { r.writeHead(404); r.end(); return; }
    r.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream', 'cache-control': 'no-store' });
    fs.createReadStream(f).pipe(r);
  });
  return new Promise((res) => srv.listen(0, '127.0.0.1', () => res(srv)));
}

const results = [];
const check = (name, ok, detail = '') => { results.push({ name, ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`); };

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await chromium.launch({
  headless: !process.env.HEADFUL,
  // Raw host candidates instead of mDNS names: CI containers have no multicast DNS.
  args: ['--disable-features=WebRtcHideLocalIpsWithMdns'],
  executablePath: process.env.CHROMIUM_PATH || undefined,
  channel: process.env.E2E_BROWSER === 'chrome' && !process.env.CHROMIUM_PATH ? 'chrome' : undefined,
});
const problems = [];
const watch = (page, who) => {
  page.on('pageerror', (e) => problems.push(`${who} pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error' || /Content Security Policy|Refused to/i.test(m.text())) problems.push(`${who} console: ${m.text()}`);
  });
};

try {
  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const a = await ctxA.newPage(); watch(a, 'alice');
  const b = await ctxB.newPage(); watch(b, 'bob');

  // Landing page links to the app.
  await a.goto(`${base}/`);
  const start = a.locator('a.start').first();
  check('landing page has Start link', (await start.getAttribute('href')) === 'app/');
  await start.click();
  await a.waitForSelector('#v-start:not([hidden])');
  check('app boots (wasm loaded)', /^anon_[0-9a-f]{6}$/.test(await a.textContent('#me')));

  // Alice: invite with max connectivity (raw host IPs; no STUN reachable in CI).
  await a.click('#v-start details summary');
  await a.selectOption('#s-privacy', '2');
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#t-link').value.includes('#i='), null, { timeout: 15000 });
  const invite = await a.inputValue('#t-link');
  const inviteBytes = Math.floor(invite.split('#i=')[1].length * 3 / 4);
  check('invite link produced', invite.startsWith(`${base}/app/#i=`), `${invite.length} chars, ${inviteBytes} B`);
  check('invite QR rendered', (await a.locator('#qr svg path').count()) === 1);

  // Bob: opens the app, sets the same mode, pastes the invite link.
  await b.goto(`${base}/app/`);
  await b.waitForSelector('#v-start:not([hidden])');
  await b.click('#v-start details summary');
  await b.selectOption('#s-privacy', '2');
  await b.fill('#t-code', invite);
  await b.click('#b-apply');
  await b.waitForFunction(() => document.querySelector('#t-link').value.includes('#a='), null, { timeout: 15000 });
  const answer = await b.inputValue('#t-link');
  check('answer link produced', answer.startsWith(`${base}/app/#a=`), `${answer.length} chars`);

  // Alice opens Bob's answer link in a new tab: it must be handed to her first tab.
  const a2 = await ctxA.newPage(); watch(a2, 'alice-tab2');
  await a2.goto(answer);
  await a2.waitForSelector('#v-note:not([hidden])', { timeout: 5000 });
  check('answer link forwarded to owning tab', (await a2.textContent('#note-title')) === 'Answer delivered');
  check('fragment scrubbed from URL', !a2.url().includes('#'));
  await a2.close();

  await Promise.all([
    a.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 }),
    b.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 }),
  ]);
  check('both sides connected', true);
  const sasA = await a.textContent('#sas-digits'); const sasB = await b.textContent('#sas-digits');
  const emA = await a.textContent('#sas-emoji'); const emB = await b.textContent('#sas-emoji');
  check('SAS equal on both sides', sasA === sasB && emA === emB && /^\d{3} \d{3}$/.test(sasA), `${sasA} ${emA}`);
  check('peer handles shown', (await a.textContent('#peer')) === (await b.textContent('#me')) && (await b.textContent('#peer')) === (await a.textContent('#me')));

  // Chat both ways, with delivery ticks.
  const msgA = 'Hello Bob — ünïcödé ✓ 🦀';
  await a.fill('#t-msg', msgA);
  await a.click('#b-send');
  await b.waitForFunction((t) => [...document.querySelectorAll('#log li.them')].some((li) => li.textContent === t), msgA, { timeout: 5000 });
  check('Alice → Bob delivered', true);
  await a.waitForSelector('#log li.me .tick.ok', { timeout: 5000 });
  check('delivery tick ✓ on sender', (await a.textContent('#log li.me .tick')) === '✓');
  await b.fill('#t-msg', 'Hi Alice');
  await b.press('#t-msg', 'Enter');
  await a.waitForFunction(() => [...document.querySelectorAll('#log li.them')].some((li) => li.textContent === 'Hi Alice'), null, { timeout: 5000 });
  check('Bob → Alice delivered (Enter to send)', true);
  const big = 'x'.repeat(4096);
  await a.fill('#t-msg', big);
  await a.click('#b-send');
  await b.waitForFunction(() => [...document.querySelectorAll('#log li.them')].some((li) => li.textContent.length === 4096), null, { timeout: 5000 });
  check('4096-byte message delivered', true);

  await b.click('#b-sas-ok');
  check('SAS confirm marks verified', (await b.textContent('#verified')) === 'verified');

  // Bob leaves → Alice sees the end.
  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  check('leave ends chat on the other side', (await a.textContent('#note-title')) === 'Chat ended', await a.textContent('#note-text'));

  // A bad code is rejected with a clear error and does not crash.
  await a.click('#b-again');
  await a.fill('#t-code', `${base}/app/#i=AAAA`);
  await a.click('#b-apply');
  await a.waitForSelector('#error:not([hidden])');
  check('invalid code rejected', /not a valid Ephem code/.test(await a.textContent('#error')));

  check('no CSP violations or page errors', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('e2e flow', false, e.message.split('\n')[0]);
  if (problems.length) console.log(problems.join('\n'));
} finally {
  await browser.close();
  srv.close();
}
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
