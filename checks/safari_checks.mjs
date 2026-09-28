// Real-Safari checkpoints (macOS). Serves checks/web/ locally and opens it in Safari.
// The page runs S1 (one tab), S2, S4, S8, E2, C-P1, C-P4 and posts results back.
// Cross-engine S1: a Playwright-driven Chrome tab is the other peer; the two browsers
// exchange only the minimal fields (as the binary codes would) through a local mailbox.
// Usage: node safari_checks.mjs <out-dir>        Env: NET=0 to skip network checks
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as url from 'node:url';
import * as cp from 'node:child_process';
import * as pw from 'playwright';
import * as mk from './make_web_config.mjs';
import * as sv from './serve.mjs';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
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

const { srv, target, mbGet, mbPut, finished } = await sv.startServer(cfg);

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
srv.close(); if (peer) await Promise.race([peer.close().catch(() => {}), new Promise((z) => setTimeout(z, 5000))]);
if (!page) { console.log('Safari checks timed out after 10 minutes'); process.exit(1); }
const results = [...page.slice(0, 2), ...crossResults, ...page.slice(2)];
sv.writeReport(OUT, 'safari', results);
process.exit(0);
