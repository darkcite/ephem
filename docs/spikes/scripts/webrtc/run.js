// S1 (SDP template reconstruction), S2 (code sizes), S4 (camera permission vs mDNS)
// in Chromium. Two separate browser contexts play Alice and Bob; only the
// minimal fields (what the binary invite/answer would carry) cross between them.
const http = require('http');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright-core');

const html = `<!doctype html><meta charset=utf-8><script src="/page.js"></script>`;
const srv = http.createServer((q, r) => {
  if (q.url === '/page.js') { r.writeHead(200, { 'content-type': 'text/javascript' }); r.end(fs.readFileSync(path.join(__dirname, 'page.js'))); }
  else { r.writeHead(200, { 'content-type': 'text/html' }); r.end(html); }
});

function codeBytes(f, kind) {
  // SPEC §8.3/§8.4 layout
  const hdr = 4;
  const body = kind === 'invite' ? 16 + 16 + 32 + 4 : 16 + 32;
  const cred = 1 + f.ufrag.length + 1 + f.pwd.length + 32;
  const cands = f.cands.reduce((a, c) => a + (/^\d+\.\d+\.\d+\.\d+$/.test(c.addr) ? 7 : 19), 0);
  return hdr + body + cred + cands;
}
const b64len = (n) => Math.ceil((n * 4) / 3);

async function scenario(name, args, opts = {}) {
  const browser = await chromium.launch({ executablePath: '/opt/pw-browsers/chromium-1194/chrome-linux/chrome',
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', ...args] });
  const url = `http://127.0.0.1:${srv.address().port}/`;
  const ca = await browser.newContext(), cb = await browser.newContext();
  if (opts.camera) await ca.grantPermissions(['camera'], { origin: url.slice(0, -1) });
  const a = await ca.newPage(), b = await cb.newPage();
  await a.goto(url); await b.goto(url);
  if (opts.camera === 'live') await a.evaluate(() => T.camera().then(() => true));
  if (opts.camera === 'stopped') await a.evaluate(() => T.cameraThenStop());

  const off = await a.evaluate(() => T.offer());
  const ans = await b.evaluate((f) => T.answer(f), off);
  await a.evaluate((f) => T.applyAnswer(f), ans);
  const [oa, ob] = await Promise.all([a.evaluate(() => T.waitOpen(15000)), b.evaluate(() => T.waitOpen(15000))]);
  let echo = null, pair = null;
  if (oa === 'open' && ob === 'open') {
    await a.evaluate(() => T.send('hello from alice ✓'));
    await b.waitForFunction(() => T.got().length > 0, null, { timeout: 5000 }).catch(() => {});
    echo = await b.evaluate(() => T.got());
    pair = await a.evaluate(() => T.selectedPair());
  }
  const res = {
    scenario: name,
    offer: { ufragLen: off.ufrag.length, pwdLen: off.pwd.length, setup: off.setup, mid: off.mid, sctpPort: off.sctpPort,
      maxMsg: off.maxMsg, gatherMs: off.gatherMs, cands: off.cands, rawSdpBytes: off.rawSdpBytes,
      inviteBytes: codeBytes(off, 'invite') },
    answer: { ufragLen: ans.ufrag.length, pwdLen: ans.pwd.length, setup: ans.setup, cands: ans.cands, rawSdpBytes: ans.rawSdpBytes,
      answerBytes: codeBytes(ans, 'answer') },
    inviteBase64urlChars: b64len(codeBytes(off, 'invite')),
    rawOfferSdpBase64Chars: b64len(off.rawSdpBytes),
    channel: { alice: oa, bob: ob, received: echo, selectedPair: pair },
  };
  if (opts.dumpSdp) res.offerSdp = off.sdp;
  await browser.close();
  return res;
}

(async () => {
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  const out = [];
  out.push(await scenario('default (mDNS on)', [], { dumpSdp: true }));
  out.push(await scenario('mDNS obfuscation disabled', ['--disable-features=WebRtcHideLocalIpsWithMdns']));
  out.push(await scenario('camera permission + live track (Alice)', [], { camera: 'live' }));
  out.push(await scenario('camera permission, track stopped (Alice)', [], { camera: 'stopped' }));
  console.log(JSON.stringify(out, null, 1));
  srv.close();
})().catch((e) => { console.error(e); process.exit(1); });
