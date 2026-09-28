// End-to-end test of the Ephem app (/app/) in real Chromium, over a real WebRTC DataChannel.
//
//   Alice: landing → app → saves her identity (key file), switches away and signs back in
//   Alice: invite (+ "what your peer sees" panel)
//   Bob (separate browser): **scans** the invite QR with a fake camera that plays the real code
//        (rqrr decoder in wasm, the iOS path) → answer
//   Alice: opens Bob's answer link in a NEW tab → handed to her first tab (BroadcastChannel, §8.7)
//   → Noise KK → equal SAS → chat both ways, ✓ delivered, ✓✓ read, typing, reply, edit, delete,
//   self-destruct timer, path diagnostics → simulated network loss → message queued (🕓) →
//   reconnect codes (T3) → queued message delivered → leave → invalid code rejected.
//
// Usage: node checks/e2e_app.mjs            (headless; build first with ./build.sh)
//        HEADFUL=1 node checks/e2e_app.mjs
//        E2E_BROWSER=chrome node checks/e2e_app.mjs   (your installed Google Chrome, no download)
// Exit code 0 = PASS. Also fails on any CSP violation or page error.
import * as fs from 'node:fs';
import * as http from 'node:http';
import * as os from 'node:os';
import * as path from 'node:path';
import * as url from 'node:url';
import { chromium } from 'playwright';
import QRCode from 'qrcode';

const ROOT = path.resolve(path.dirname(url.fileURLToPath(import.meta.url)), '..');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.png': 'image/png' };
const PASS = 'correct horse battery staple';

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

/** A Y4M video (for Chrome's fake camera) showing `text` as a QR code. */
function qrVideo(text, file) {
  const q = QRCode.create(text, { errorCorrectionLevel: 'M' });
  const n = q.modules.size;
  const [W, H] = [640, 480];
  const scale = Math.floor((H * 0.9) / (n + 8));
  const x0 = Math.floor((W - n * scale) / 2);
  const y0 = Math.floor((H - n * scale) / 2);
  const Y = Buffer.alloc(W * H, 255);
  for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) {
    if (!q.modules.get(r, c)) continue;
    for (let dy = 0; dy < scale; dy++) Y.fill(0, (y0 + r * scale + dy) * W + x0 + c * scale, (y0 + r * scale + dy) * W + x0 + (c + 1) * scale);
  }
  const UV = Buffer.alloc((W / 2) * (H / 2) * 2, 128);
  const frame = Buffer.concat([Buffer.from('FRAME\n'), Y, UV]);
  fs.writeFileSync(file, Buffer.concat([Buffer.from(`YUV4MPEG2 W${W} H${H} F10:1 Ip A1:1 C420jpeg\n`), frame, frame]));
}

const results = [];
const check = (name, ok, detail = '') => { results.push({ name, ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`); };
const launch = (args = []) => chromium.launch({
  headless: !process.env.HEADFUL,
  // Raw host candidates instead of mDNS names: CI containers have no multicast DNS.
  args: ['--disable-features=WebRtcHideLocalIpsWithMdns', ...args],
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
const lastThem = (page) => page.locator('#log li.them').last();
const msgWith = (page, cls, t) => page.locator(`#log li.${cls}`, { hasText: t }).first();
const openSettings = async (page) => { await page.click('#v-start details summary'); await page.selectOption('#s-privacy', '2'); };

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}`;
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ephem-e2e-'));
const browserA = await launch();
let browserB = null;

try {
  const ctxA = await browserA.newContext({ acceptDownloads: true });
  const a = await ctxA.newPage(); watch(a, 'alice');

  // ---- landing and boot ----
  await a.goto(`${base}/`);
  const start = a.locator('a.start').first();
  check('landing page has Start link', (await start.getAttribute('href')) === 'app/');
  await start.click();
  await a.waitForSelector('#v-start:not([hidden])');
  const temp1 = await a.textContent('#me');
  check('app boots with a temporary identity', /^anon_[0-9a-f]{6}$/.test(temp1));

  // ---- identity key file (§7.3) ----
  await a.click('#b-id-save');
  await a.fill('#i-label', 'Laptop');
  await a.fill('#i-pass', PASS);
  await a.fill('#i-pass2', PASS);
  const [dl] = await Promise.all([a.waitForEvent('download'), a.click('#b-id-do-save')]);
  const keyText = await a.inputValue('#t-keytext');
  check('identity saved as encrypted key file', dl.suggestedFilename() === 'ephem-Laptop.p2pkey' && keyText.length > 100, `${dl.suggestedFilename()}, ${keyText.length} chars`);
  const savedHandle = await a.textContent('#me');
  check('saved identity keeps the handle', savedHandle === temp1 && /Laptop/.test(await a.textContent('#id-desc')));
  await a.click('#b-id-temp');
  const temp2 = await a.textContent('#me');
  check('switch to a new temporary identity', temp2 !== savedHandle);
  await a.click('#b-id-load');
  await a.fill('#t-keyin', keyText);
  await a.fill('#i-pass-in', 'wrong passphrase!!');
  await a.click('#b-id-do-load');
  await a.waitForSelector('#error:not([hidden])');
  check('wrong passphrase rejected', /Wrong passphrase/.test(await a.textContent('#error')) && (await a.textContent('#me')) === temp2);
  await a.fill('#i-pass-in', PASS);
  await a.click('#b-id-do-load');
  await a.waitForFunction((h) => document.querySelector('#me').textContent === h, savedHandle, { timeout: 10000 });
  check('sign in with key text restores the identity', true, savedHandle);

  // ---- invite ----
  await openSettings(a);
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link').value.includes('#i='), null, { timeout: 15000 });
  const invite = await a.inputValue('#v-code .link');
  check('invite link produced', invite.startsWith(`${base}/app/#i=`), `${invite.length} chars, ${Math.floor(invite.split('#i=')[1].length * 3 / 4)} B`);
  check('invite QR rendered', (await a.locator('#v-code .qr svg path').count()) === 1);
  await a.waitForSelector('#exposure:not([hidden])');
  check('"what your peer sees" panel shown (§29.2)', (await a.textContent('#exposure .addrs')).length > 0, await a.textContent('#exposure .addrs'));

  // ---- Bob scans the invite with a camera ----
  const video = path.join(tmp, 'invite.y4m');
  qrVideo(invite, video);
  browserB = await launch(['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-video-capture=${video}`]);
  const ctxB = await browserB.newContext({ permissions: ['camera'] });
  const b = await ctxB.newPage(); watch(b, 'bob');
  await b.goto(`${base}/app/`);
  await b.waitForSelector('#v-start:not([hidden])');
  await openSettings(b);
  await b.click('#b-scan');
  await b.waitForFunction(() => document.querySelector('#v-code .link').value.includes('#a='), null, { timeout: 20000 });
  check('invite scanned from camera (wasm QR decoder)', await b.locator('#scanner').isHidden());
  const answer = await b.inputValue('#v-code .link');
  check('answer link produced', answer.startsWith(`${base}/app/#a=`), `${answer.length} chars`);

  // ---- Alice opens the answer link in a new tab: handed to her first tab ----
  const a2 = await ctxA.newPage(); watch(a2, 'alice-tab2');
  await a2.goto(answer);
  await a2.waitForSelector('#v-note:not([hidden])', { timeout: 5000 });
  check('answer link forwarded to owning tab', (await a2.textContent('#note-title')) === 'Code delivered');
  check('fragment scrubbed from URL', !a2.url().includes('#'));
  await a2.close();

  await Promise.all([a.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 }), b.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 })]);
  const sasA = await a.textContent('#sas-digits');
  check('both sides connected, SAS equal', sasA === (await b.textContent('#sas-digits')) && (await a.textContent('#sas-emoji')) === (await b.textContent('#sas-emoji')), `${sasA} ${await a.textContent('#sas-emoji')}`);
  check('Bob sees Alice\'s saved identity', (await b.textContent('#peer')) === savedHandle);

  // ---- chat, ticks, typing ----
  const m1 = 'Hello Bob — ünïcödé ✓ 🦀';
  await a.fill('#t-msg', m1);
  await a.click('#b-send');
  await msgWith(b, 'them', m1).waitFor({ timeout: 5000 });
  check('Alice → Bob delivered', true);
  await a.waitForFunction(() => document.querySelector('#log li.me .tick')?.textContent === '✓✓', null, { timeout: 8000 });
  check('ticks ✓ then ✓✓ (read receipt)', true);
  await b.locator('#t-msg').pressSequentially('typing', { delay: 20 });
  await a.waitForFunction(() => document.querySelector('#peer-state').textContent === 'typing…', null, { timeout: 5000 });
  check('typing indicator', true);
  await b.fill('#t-msg', '');

  // Reply
  await msgWith(b, 'them', m1).click();
  await b.click('#log .acts button:text("Reply")');
  await b.fill('#t-msg', 'Hi Alice');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'Hi Alice').waitFor({ timeout: 5000 });
  check('reply shows the quoted message', /You: Hello Bob/.test(await lastThem(a).locator('.quote').textContent()));

  // Edit
  await msgWith(a, 'me', m1).click();
  await a.click('#log .acts button:text("Edit")');
  await a.fill('#t-msg', 'Hello Bob (edited)');
  await a.press('#t-msg', 'Enter');
  await b.waitForFunction(() => [...document.querySelectorAll('#log li.them')].some((li) => li.textContent.includes('Hello Bob (edited)') && li.textContent.includes('edited')), null, { timeout: 5000 });
  check('edit reaches the peer', true);

  // Delete for everyone
  await a.fill('#t-msg', 'oops, wrong chat');
  await a.press('#t-msg', 'Enter');
  await msgWith(b, 'them', 'oops').waitFor({ timeout: 5000 });
  await msgWith(a, 'me', 'oops').click();
  await a.click('#log .acts button:text("Delete for everyone")');
  await b.waitForFunction(() => [...document.querySelectorAll('#log li.them.deleted')].some((li) => li.textContent.includes('Message deleted')), null, { timeout: 5000 });
  check('delete for everyone', !(await b.textContent('#log')).includes('oops'));

  // 4 KiB message
  await a.fill('#t-msg', 'x'.repeat(4096));
  await a.click('#b-send');
  await b.waitForFunction(() => [...document.querySelectorAll('#log li.them .body')].some((s) => s.textContent.length === 4096), null, { timeout: 5000 });
  check('4096-byte message delivered', true);

  // Self-destruct timer
  await a.selectOption('#s-chat-ttl', '5');
  await b.waitForFunction(() => document.querySelector('#s-chat-ttl').value === '5', null, { timeout: 5000 });
  check('self-destruct setting reaches the peer', /disappear after 5 seconds/.test(await b.textContent('#log')));
  await a.fill('#t-msg', 'this will vanish');
  await a.press('#t-msg', 'Enter');
  await msgWith(b, 'them', 'this will vanish').waitFor({ timeout: 5000 });
  const t0 = Date.now();
  await b.waitForFunction(() => !document.querySelector('#log').textContent.includes('this will vanish'), null, { timeout: 12000 });
  await a.waitForFunction(() => !document.querySelector('#log').textContent.includes('this will vanish'), null, { timeout: 12000 });
  check('self-destructing message removed on both sides', true, `${((Date.now() - t0) / 1000).toFixed(1)} s after arrival`);
  await b.selectOption('#s-chat-ttl', '0');
  await a.waitForFunction(() => document.querySelector('#s-chat-ttl').value === '0', null, { timeout: 5000 });

  // Diagnostics
  await a.click('#b-info');
  await a.waitForFunction(() => document.querySelector('#diag-path').textContent.includes('↔'), null, { timeout: 12000 });
  check('path diagnostics (getStats, no relay)', true, await a.textContent('#diag-path'));

  // ---- network loss and T3 reconnect ----
  await a.click('#b-drop');
  await a.waitForSelector('#resume:not([hidden])');
  await b.waitForSelector('#resume:not([hidden])', { timeout: 20000 });
  check('both sides detect the lost path', true);
  await a.fill('#t-msg', 'sent while offline');
  await a.press('#t-msg', 'Enter');
  check('message queued while offline (🕓)', (await msgWith(a, 'me', 'sent while offline').locator('.tick').textContent()) === '🕓');
  await a.click('#b-resume');
  await a.waitForFunction(() => document.querySelector('#resume .link').value.includes('#r='), null, { timeout: 15000 });
  await b.fill('#t-resume', await a.inputValue('#resume .link'));
  await b.click('#b-resume-apply');
  await b.waitForFunction(() => document.querySelector('#resume .link').value.includes('#q='), null, { timeout: 15000 });
  await a.fill('#t-resume', await b.inputValue('#resume .link'));
  await a.click('#b-resume-apply');
  await msgWith(b, 'them', 'sent while offline').waitFor({ timeout: 20000 });
  check('reconnected with a reconnect code; queued message delivered', await a.locator('#resume').isHidden());
  await a.waitForFunction(() => [...document.querySelectorAll('#log li.me')].some((li) => li.textContent.includes('sent while offline') && /✓/.test(li.querySelector('.tick').textContent)), null, { timeout: 8000 });
  check('queued message ticked after reconnect', true);
  await b.fill('#t-msg', 'still here');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'still here').waitFor({ timeout: 5000 });
  check('chat continues after reconnect', true);

  await b.click('#b-sas-ok');
  check('SAS confirm marks verified', (await b.textContent('#verified')) === 'verified');

  // ---- leave ----
  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  check('leave ends the chat on the other side', (await a.textContent('#note-title')) === 'Chat ended', await a.textContent('#note-text'));

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
  await browserA.close();
  await browserB?.close();
  srv.close();
  fs.rmSync(tmp, { recursive: true, force: true });
}
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
