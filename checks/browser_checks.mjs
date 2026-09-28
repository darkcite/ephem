// Browser checkpoints: S1, S2, S4, S8/TS3/TS4, E2 (G2), C-P1, C-P4, E8.
// Usage: node browser_checks.mjs <out-dir>
// Env:   BROWSERS=chrome,webkit (chrome = installed Google Chrome; chromium/firefox/webkit = Playwright builds)
//        NET=1 (live network checks)   E8=1 (7-minute hidden-tab test, headed)
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as http from 'node:http';
import * as url from 'node:url';
import * as pw from 'playwright';
import * as ipns from 'ipns';
import * as keys from '@libp2p/crypto/keys';
import * as pid from '@libp2p/peer-id';
import * as b36 from 'multiformats/bases/base36';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const OUT = process.argv[2] || path.join(HERE, 'out', 'manual');
const BROWSERS = (process.env.BROWSERS || 'chrome').split(',').filter(Boolean);
const ENGINE = (k) => (k === 'chrome' ? 'chromium' : k);
const NET = process.env.NET !== '0';
const E8 = process.env.E8 === '1';
fs.mkdirSync(OUT, { recursive: true });

const SNOWFLAKE_FP = '2B280B23E1107BB62ABFC40DDCC8824814F80A72';
const SNOWFLAKE_BROKERS = ['https://1098762253.rsc.cdn77.org/', 'https://snowflake-broker.torproject.net/'];
const SNOWFLAKE_STUN = [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.antisip.com:3478', 'stun:stun.nextcloud.com:3478'] }];
const DEFAULT_STUN = [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.cloudflare.com:3478'] }];
// ipfs.io and dweb.link 301-redirect trustless requests here without CORS headers (C-P1),
// so browsers cannot use them; they are aliases of this gateway.
const GATEWAYS = ['https://trustless-gateway.link'];
const TEST_CID = 'bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi';
const DELEGATED = 'https://delegated-ipfs.dev';

// ---------- local static server (http://127.0.0.1 is a secure context) ----------
const pageJs = fs.readFileSync(path.join(HERE, 'web', 'page.js'));
const srv = http.createServer((q, r) => {
  if (q.url === '/page.js') { r.writeHead(200, { 'content-type': 'text/javascript' }); r.end(pageJs); return; }
  r.writeHead(200, { 'content-type': 'text/html' });
  r.end('<!doctype html><meta charset=utf-8><title>checks</title><script src="/page.js"></script>');
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const ORIGIN = `http://127.0.0.1:${srv.address().port}`;

const results = [];
const rec = (id, name, status, details) => { results.push({ id, name, status, details }); console.log(`[${status}] ${id} ${name}: ${typeof details === 'string' ? details : JSON.stringify(details)}`); };

function launchOpts(kind, { camera } = {}) {
  const exe = process.env[`${kind.toUpperCase()}_PATH`];
  const o = { headless: true };
  if (exe) o.executablePath = exe;
  if (kind === 'chrome') { o.args = ['--use-fake-device-for-media-stream']; if (!exe) o.channel = 'chrome'; }
  if (kind === 'chromium') { o.args = ['--use-fake-device-for-media-stream']; if (!exe) o.channel = 'chromium'; }
  if (kind === 'firefox') o.firefoxUserPrefs = { 'media.navigator.streams.fake': true, 'media.navigator.permission.disabled': !!camera };
  return o;
}
async function openPage(kind, opts = {}) {
  const browser = await pw[ENGINE(kind)].launch(launchOpts(kind, opts));
  const ctx = await browser.newContext();
  if (opts.grant && kind !== 'firefox') await ctx.grantPermissions(['camera'], { origin: ORIGIN });
  const page = await ctx.newPage();
  await page.goto(ORIGIN + '/');
  return { browser, ctx, page };
}
const isIp = (a) => /^[0-9.]+$/.test(a) || a.includes(':');
const codeBytes = (f, kind) => 4 + (kind === 'invite' ? 68 : 48) + 1 + f.ufrag.length + 1 + f.pwd.length + 32
  + f.cands.reduce((a, c) => a + (/^[0-9.]+$/.test(c.addr) ? 7 : 19), 0);

// ---------- which browsers launch at all ----------
const avail = [];
for (const b of BROWSERS) {
  try { const { browser, page } = await openPage(b); const ua = await page.evaluate(() => C.ua()); await browser.close(); avail.push(b); rec('ENV', `${b} launches`, 'PASS', ua); }
  catch (e) { rec('ENV', `${b} launches`, 'SKIP', String(e.message).split('\n')[0]); }
}

// ---------- S2: code sizes per browser ----------
for (const b of avail) {
  try {
    const { browser, page } = await openPage(b);
    const f = await page.evaluate(() => C.offer([]));
    await browser.close();
    rec('S2', `${b} offer fields`, 'PASS', { ufrag: f.ufrag.length, pwd: f.pwd.length, setup: f.setup, mid: f.mid, sctpPort: f.sctpPort,
      maxMsg: f.maxMsg, cands: f.cands.map((c) => `${c.typ}:${isIp(c.addr) ? 'ip' : 'mdns'}`), inviteBytes: codeBytes(f, 'invite'), rawSdpBytes: f.rawSdpBytes, gatherMs: f.gatherMs });
  } catch (e) { rec('S2', `${b} offer fields`, 'FAIL', e.message.split('\n')[0]); }
}

// ---------- S1: template-rebuilt SDP, full offerer x answerer matrix ----------
for (const a of avail) for (const b of avail) {
  let A, B;
  try {
    A = await openPage(a); B = await openPage(b);
    const off = await A.page.evaluate(() => C.offer([]));
    const ans = await B.page.evaluate((f) => C.answer(f), off);
    await A.page.evaluate((f) => C.applyAnswer(f), ans);
    const [oa, ob] = await Promise.all([A.page.evaluate(() => C.waitOpen(20000)), B.page.evaluate(() => C.waitOpen(20000))]);
    let got = [];
    if (oa === 'open' && ob === 'open') {
      await A.page.evaluate(() => C.send('p2p ✓ ' + navigator.userAgent.length));
      await B.page.waitForFunction(() => C.got().length > 0, null, { timeout: 5000 }).catch(() => {});
      got = await B.page.evaluate(() => C.got());
    }
    rec('S1', `${a} → ${b}`, got.length ? 'PASS' : 'FAIL', { alice: oa, bob: ob, received: got.length, answerSetup: ans.setup });
  } catch (e) { rec('S1', `${a} → ${b}`, 'FAIL', e.message.split('\n')[0]); }
  finally { if (A) await A.browser.close(); if (B) await B.browser.close(); }
}

// ---------- S1-srflx: connect through the public (or VPN) address only ----------
// Host candidates are dropped, so the path must go out through the NAT/VPN and back in.
// Both peers are on this machine, so success also needs the NAT/VPN to allow hairpinning:
// a FAIL here is a hint, not proof, that two peers behind this NAT/VPN cannot connect.
if (NET) {
  const k = avail[0];
  let A, B;
  try {
    A = await openPage(k); B = await openPage(k);
    const off = await A.page.evaluate((s) => C.offer(s), DEFAULT_STUN);
    const ans = await B.page.evaluate(([f, s]) => C.answer(f, s, 'srflx'), [off, DEFAULT_STUN]);
    const nOff = off.cands.filter((c) => c.typ === 'srflx').length, nAns = ans.cands.filter((c) => c.typ === 'srflx').length;
    if (!nOff || !nAns) {
      rec('S1', `${k} → ${k} via srflx only`, 'INCONCLUSIVE', { srflxOffer: nOff, srflxAnswer: nAns, note: 'no srflx candidate (STUN unreachable?)' });
    } else {
      await A.page.evaluate((f) => C.applyAnswer(f, 'srflx'), ans);
      const [oa, ob] = await Promise.all([A.page.evaluate(() => C.waitOpen(20000)), B.page.evaluate(() => C.waitOpen(20000))]);
      rec('S1', `${k} → ${k} via srflx only (hairpin through NAT/VPN)`, oa === 'open' && ob === 'open' ? 'PASS' : 'FAIL',
        { alice: oa, bob: ob, srflx: [...new Set(off.cands.filter((c) => c.typ === 'srflx').map((c) => c.addr))] });
    }
  } catch (e) { rec('S1', `${k} via srflx only`, 'FAIL', e.message.split('\n')[0]); }
  finally { if (A) await A.browser.close(); if (B) await B.browser.close(); }
}

// ---------- S4: camera permission vs mDNS host obfuscation ----------
for (const b of avail) {
  const scen = [['no permission', {}, null], ['permission granted, camera never opened', { grant: true, camera: true }, null],
                ['camera opened and stopped', { grant: true, camera: true }, false], ['camera live', { grant: true, camera: true }, true]];
  for (const [name, opts, cam] of scen) {
    if (b === 'firefox' && name.startsWith('permission granted')) { rec('S4', `${b}: ${name}`, 'N/A', 'Firefox has no persistent grant in this harness'); continue; }
    let P;
    try {
      P = await openPage(b, opts);
      if (cam !== null) await P.page.evaluate((keep) => C.camera(keep), cam);
      const f = await P.page.evaluate(() => C.offer([]));
      const kinds = [...new Set(f.cands.filter((c) => c.typ === 'host').map((c) => (isIp(c.addr) ? 'RAW-IP' : 'mdns')))];
      rec('S4', `${b}: ${name}`, 'INFO', `host candidates: ${kinds.join(',') || 'none'}`);
    } catch (e) { rec('S4', `${b}: ${name}`, 'FAIL', e.message.split('\n')[0]); }
    finally { if (P) await P.browser.close(); }
  }
}

if (NET) {
  // ---------- S8 / TS3 / TS4: public addresses a peer would see ----------
  for (const b of avail) {
    let P;
    try {
      P = await openPage(b);
      const r = await P.page.evaluate((s) => C.srflx(s), DEFAULT_STUN);
      const v4 = r.srflx.filter((a) => !a.includes(':')), v6 = r.srflx.filter((a) => a.includes(':'));
      rec('S8', `${b} srflx via Google+Cloudflare STUN`, r.srflx.length ? 'PASS' : 'FAIL', { v4, v6, gatherComplete: r.complete });
      if (v4.length && v6.length) rec('TS4', `${b} both IPv4 and IPv6 visible`, 'INFO', 'If you are on a v4-only VPN, the IPv6 address above is your real one (P2P-CHAT.md §29.2)');
    } catch (e) { rec('S8', `${b} srflx`, 'FAIL', e.message.split('\n')[0]); }
    finally { if (P) await P.browser.close(); }
  }

  // ---------- E2 / gate G2: live Snowflake rendezvous + DataChannel to a proxy ----------
  for (const b of avail) for (const broker of SNOWFLAKE_BROKERS) {
    let P;
    try {
      P = await openPage(b);
      let r = null;
      for (let attempt = 1; attempt <= 3 && !(r && r.ok); attempt++) {
        r = await P.page.evaluate(([u, fp, s]) => C.snowflake(u, fp, s, 20000), [broker, SNOWFLAKE_FP, SNOWFLAKE_STUN]);
        r.attempt = attempt;
      }
      rec('E2', `${b} via ${new URL(broker).host}`, r.ok ? 'PASS' : 'FAIL', r);
    } catch (e) { rec('E2', `${b} via ${broker}`, 'FAIL', e.message.split('\n')[0]); }
    finally { if (P) await P.browser.close(); }
  }

  // ---------- C-P1: trustless CAR from public gateways (CORS) ----------
  for (const b of avail) {
    let P;
    try {
      P = await openPage(b);
      for (const gw of GATEWAYS) {
        const r = await P.page.evaluate(([g, c]) => C.gwCar(g, c), [gw, TEST_CID]);
        rec('C-P1', `${b} CAR from ${new URL(gw).host}`, r.ok ? 'PASS' : 'FAIL', r);
      }
    } catch (e) { rec('C-P1', `${b}`, 'FAIL', e.message.split('\n')[0]); }
    finally { if (P) await P.browser.close(); }
  }

  // ---------- C-P4: browser publishes a signed IPNS record, gateways serve it back ----------
  try {
    const priv = await keys.generateKeyPair('Ed25519');
    const name = pid.peerIdFromPrivateKey(priv).toCID().toString(b36.base36);
    const record = ipns.marshalIPNSRecord(await ipns.createIPNSRecord(priv, `/ipfs/${TEST_CID}`, 1n, 60 * 60 * 1000));
    const b64 = Buffer.from(record).toString('base64');
    const P = await openPage(avail[0]);
    const put = await P.page.evaluate(([u, n, r]) => C.ipnsPut(u, n, r), [DELEGATED, name, b64]);
    rec('C-P4', `${avail[0]} PUT IPNS record to ${new URL(DELEGATED).host}`, put.ok ? 'PASS' : 'FAIL', { name, ...put });
    if (put.ok) {
      for (const gw of GATEWAYS) {
        let r = null;
        for (let i = 0; i < 4 && !(r && r.ok); i++) { if (i) await new Promise((z) => setTimeout(z, 15000)); r = await P.page.evaluate(([g, n, x]) => C.ipnsGet(g, n, x), [gw, name, b64]); }
        rec('C-P4', `read back via ${new URL(gw).host}`, r.ok ? 'PASS' : 'FAIL', r);
      }
    }
    await P.browser.close();
  } catch (e) { rec('C-P4', 'IPNS publish', 'FAIL', e.message.split('\n')[0]); }
}

// ---------- E8: hidden tab with an open DataChannel (Chromium, headed, ~7 min) ----------
const e8kind = avail.find((k) => ENGINE(k) === 'chromium');
if (E8 && e8kind) {
  // Playwright normally disables background throttling; E8 must measure the real behaviour.
  const browser = await pw.chromium.launch({ ...launchOpts(e8kind), headless: false,
    ignoreDefaultArgs: ['--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding'] });
  try {
    const ctx = await browser.newContext();
    const a = await ctx.newPage(); await a.goto(ORIGIN + '/');
    await a.evaluate(() => C.e8Start());
    // Hide tab A. 1) open a second tab from A (same window, so A goes to the background);
    // 2) if A still reports 'visible', minimize its window through the DevTools protocol.
    const [b] = await Promise.all([ctx.waitForEvent('page'), a.evaluate((u) => { window.open(u, '_blank'); }, ORIGIN + '/')]);
    await b.bringToFront();
    await new Promise((z) => setTimeout(z, 2000));
    let method = 'second tab in the same window';
    if (await a.evaluate(() => document.visibilityState) !== 'hidden') {
      const cdp = await ctx.newCDPSession(a);
      const { windowId } = await cdp.send('Browser.getWindowForTarget');
      await cdp.send('Browser.setWindowBounds', { windowId, bounds: { windowState: 'minimized' } });
      await new Promise((z) => setTimeout(z, 2000));
      method = 'window minimized';
    }
    const vis = await a.evaluate(() => document.visibilityState);
    console.log(`E8: tab A visibility=${vis} (${method}); waiting 7 minutes...`);
    await new Promise((z) => setTimeout(z, 7 * 60 * 1000));
    const r = { ...(await a.evaluate(() => C.e8Result())), hiddenBy: method };
    const status = r.hiddenSamples === 0 ? 'INCONCLUSIVE' : (r.maxGapMs < 5000 ? 'PASS' : 'FAIL');
    rec('E8', 'hidden tab timers with open DataChannel', status, { ...r, note: 'PASS = timers kept running (max gap < 5 s) while hidden; intensive throttling would give ~60 s gaps' });
  } catch (e) { rec('E8', 'hidden tab', 'FAIL', e.message.split('\n')[0]); }
  finally { await browser.close(); }
}

srv.close();
fs.writeFileSync(path.join(OUT, 'browser.json'), JSON.stringify(results, null, 1));
const md = ['| ID | Check | Result | Details |', '|---|---|---|---|',
  ...results.map((r) => `| ${r.id} | ${r.name} | ${r.status} | ${(typeof r.details === 'string' ? r.details : JSON.stringify(r.details)).replace(/\|/g, '\\|').slice(0, 400)} |`)];
fs.writeFileSync(path.join(OUT, 'browser.md'), md.join('\n') + '\n');
