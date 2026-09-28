// Real-Safari checkpoints (macOS). Serves checks/web/ locally and opens it in Safari.
// The page runs S1 (one tab), S2, S4, S8, E2, C-P1, C-P4 and posts results back.
// Cross-engine S1: a Playwright-driven Chrome tab is the other peer; the two browsers
// exchange only the minimal fields (as the binary codes would) through a local mailbox.
// Usage: node safari_checks.mjs <out-dir>        Env: NET=0 to skip network checks
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as http from 'node:http';
import * as url from 'node:url';
import * as cp from 'node:child_process';
import * as pw from 'playwright';
import * as mk from './make_web_config.mjs';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const WEB = path.join(HERE, 'web');
const OUT = process.argv[2] || path.join(HERE, 'out', 'safari');
fs.mkdirSync(OUT, { recursive: true });

// ---------- peer browser for the cross-engine test (installed Chrome, else Playwright Chromium) ----------
let peer = null;
for (const channel of ['chrome', 'chromium']) {
  try { peer = await pw.chromium.launch({ channel, headless: true }); peer.channelName = channel; break; } catch (_) { /* try next */ }
}

const cfg = await mk.baseConfig({
  label: 'safari', net: process.env.NET !== '0', autorun: true, interactive: false, cross: !!peer, resultUrl: 'result',
});
if (!cfg.net) delete cfg.ipnsName;

// ---------- tiny server: static files, config, mailbox, results ----------
const box = new Map(); const waiters = new Map();
const mbPut = (k, v) => { box.set(k, v); (waiters.get(k) || []).forEach((w) => w(v)); waiters.delete(k); };
const mbGet = (k, ms = 60000) => new Promise((res, rej) => {
  if (box.has(k)) return res(box.get(k));
  const t = setTimeout(() => rej(new Error(`mailbox timeout: ${k}`)), ms);
  waiters.set(k, [...(waiters.get(k) || []), (v) => { clearTimeout(t); res(v); }]);
});
let done; const finished = new Promise((r) => { done = r; });
const types = { '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json' };
const srv = http.createServer((q, r) => {
  const u = new URL(q.url, 'http://x');
  const body = () => new Promise((res) => { let b = ''; q.on('data', (d) => { b += d; }); q.on('end', () => res(b)); });
  if (u.pathname === '/peer.html') { r.writeHead(200, { 'content-type': 'text/html' }); r.end('<!doctype html><meta charset=utf-8><script src="page.js"></script>'); return; }
  if (u.pathname === '/config.json') { r.writeHead(200, { 'content-type': 'application/json' }); r.end(JSON.stringify(cfg)); return; }
  if (u.pathname.startsWith('/mb/')) {
    const k = u.pathname.slice(4);
    if (q.method === 'POST') { body().then((b) => { mbPut(k, JSON.parse(b)); r.end('ok'); }); return; }
    mbGet(k, 25000).then((v) => { r.writeHead(200, { 'content-type': 'application/json' }); r.end(JSON.stringify(v)); })
      .catch(() => { r.writeHead(204); r.end(); });
    return;
  }
  if (u.pathname === '/result' && q.method === 'POST') { body().then((b) => { r.end('ok'); if (u.searchParams.get('final')) done(JSON.parse(b)); }); return; }
  const f = path.join(WEB, u.pathname === '/' ? 'index.html' : path.normalize(u.pathname).replace(/^\/+/, ''));
  if (!f.startsWith(WEB) || !fs.existsSync(f)) { r.writeHead(404); r.end(); return; }
  r.writeHead(200, { 'content-type': types[path.extname(f)] || 'application/octet-stream' }); r.end(fs.readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const target = `http://127.0.0.1:${srv.address().port}/`;

// ---------- cross-engine orchestration (runs while the Safari page works through its list) ----------
const crossResults = [];
async function crossEngine() {
  const p = await (await peer.newContext()).newPage();
  await p.goto(target + 'peer.html');
  await p.waitForFunction(() => window.C);
  const name = peer.channelName;
  // A: Safari offers, peer answers.
  const offA = await mbGet('offerA', 120000);
  mbPut('answerA', await p.evaluate((f) => C.answer(f), offA));
  const openA = await p.evaluate(() => C.waitOpen(20000));
  await p.waitForFunction(() => C.got().length > 0, null, { timeout: 8000 }).catch(() => {});
  const gotA = await p.evaluate(() => C.got());
  const doneA = await mbGet('doneA');
  crossResults.push({ id: 'S1', name: `safari → ${name}`, status: gotA.some((m) => m.startsWith('from safari')) ? 'PASS' : 'FAIL', details: { safari: doneA.open, [name]: openA, received: gotA } });
  // B: peer offers, Safari answers.
  mbPut('offerB', await p.evaluate(() => C.offer([])));
  await p.evaluate((a) => C.applyAnswer(a), await mbGet('answerB'));
  const openB = await p.evaluate(() => C.waitOpen(20000));
  if (openB === 'open') await p.evaluate(() => C.send('from chrome'));
  const doneB = await mbGet('doneB');
  crossResults.push({ id: 'S1', name: `${name} → safari`, status: doneB.got.includes('from chrome') ? 'PASS' : 'FAIL', details: { [name]: openB, safari: doneB.open, received: doneB.got } });
}
const crossRun = peer ? crossEngine().catch((e) => crossResults.push({ id: 'S1', name: 'cross-engine', status: 'FAIL', details: e.message })) : Promise.resolve();

console.log(`Opening ${target} in Safari (if nothing opens, paste the URL into Safari yourself)`);
if (process.platform === 'darwin') cp.spawn('open', ['-a', 'Safari', target], { stdio: 'ignore' });
if (!peer) console.log('note: no Chrome/Chromium for the cross-engine test; it is skipped');

const timeout = new Promise((r) => setTimeout(() => r(null), 10 * 60 * 1000));
const page = await Promise.race([finished, timeout]);
await Promise.race([crossRun, new Promise((r) => setTimeout(r, 5000))]);
srv.close(); if (peer) await peer.close();
if (!page) { console.log('Safari checks timed out after 10 minutes'); process.exit(1); }
const results = [...page.slice(0, 2), ...crossResults, ...page.slice(2)];
for (const x of results) console.log(`[${x.status}] ${x.id} ${x.name}: ${typeof x.details === 'string' ? x.details : JSON.stringify(x.details)}`);
fs.writeFileSync(path.join(OUT, 'safari.json'), JSON.stringify(results, null, 1));
const md = ['| ID | Check | Result | Details |', '|---|---|---|---|',
  ...results.map((r) => `| ${r.id} | ${r.name} | ${r.status} | ${(typeof r.details === 'string' ? r.details : JSON.stringify(r.details)).replace(/\|/g, '\\|').slice(0, 400)} |`)];
fs.writeFileSync(path.join(OUT, 'safari.md'), md.join('\n') + '\n');
process.exit(0);
