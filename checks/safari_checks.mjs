// Real-Safari checkpoints (macOS). Opens a Safari tab on a local page; the page runs
// S1 (single tab), S2, S4 (no permission), S8, E2, C-P1, C-P4 and posts results back.
// Usage: node safari_checks.mjs <out-dir>        Env: NET=0 to skip network checks
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as http from 'node:http';
import * as url from 'node:url';
import * as cp from 'node:child_process';
import * as ipns from 'ipns';
import * as keys from '@libp2p/crypto/keys';
import * as pid from '@libp2p/peer-id';
import * as b36 from 'multiformats/bases/base36';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const OUT = process.argv[2] || path.join(HERE, 'out', 'safari');
const NET = process.env.NET !== '0';
fs.mkdirSync(OUT, { recursive: true });

const TEST_CID = 'bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi';
const cfg = {
  net: NET,
  cid: TEST_CID,
  gateways: ['https://trustless-gateway.link', 'https://ipfs.io', 'https://dweb.link'],
  delegated: 'https://delegated-ipfs.dev',
  stun: [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.cloudflare.com:3478'] }],
  snowflake: {
    fp: '2B280B23E1107BB62ABFC40DDCC8824814F80A72',
    brokers: ['https://1098762253.rsc.cdn77.org/', 'https://snowflake-broker.torproject.net/'],
    stun: [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.antisip.com:3478', 'stun:stun.nextcloud.com:3478'] }],
  },
};
if (NET) {
  const priv = await keys.generateKeyPair('Ed25519');
  cfg.ipnsName = pid.peerIdFromPrivateKey(priv).toCID().toString(b36.base36);
  const rec = ipns.marshalIPNSRecord(await ipns.createIPNSRecord(priv, `/ipfs/${TEST_CID}`, 1n, 60 * 60 * 1000));
  cfg.ipnsRecord = Buffer.from(rec).toString('base64');
}

const runner = `
const cfg = ${JSON.stringify(cfg)};
const log = (m) => { document.getElementById('log').textContent += m + '\\n'; };
const res = [];
const add = (id, name, status, details) => { res.push({ id, name, status, details }); log('[' + status + '] ' + id + ' ' + name + ': ' + JSON.stringify(details)); };
const isIp = (a) => /^[0-9.]+$/.test(a) || a.includes(':');
(async () => {
  add('ENV', 'safari', 'INFO', navigator.userAgent);
  try { const r = await C.selfPair(); add('S1', 'safari → safari (one tab)', r.ok ? 'PASS' : 'FAIL', r); } catch (e) { add('S1', 'safari one tab', 'FAIL', String(e)); }
  try { const f = await C.offer([]); add('S2', 'safari offer fields', 'PASS', { ufrag: f.ufrag.length, pwd: f.pwd.length, setup: f.setup, mid: f.mid, sctpPort: f.sctpPort, maxMsg: f.maxMsg, cands: f.cands.map((c) => c.typ + ':' + (isIp(c.addr) ? 'ip' : 'mdns')), rawSdpBytes: f.rawSdpBytes });
        add('S4', 'safari: no permission', 'INFO', 'host candidates: ' + ([...new Set(f.cands.filter((c) => c.typ === 'host').map((c) => isIp(c.addr) ? 'RAW-IP' : 'mdns'))].join(',') || 'none')); }
  catch (e) { add('S2', 'safari offer', 'FAIL', String(e)); }
  if (cfg.net) {
    try { const r = await C.srflx(cfg.stun); add('S8', 'safari srflx via Google+Cloudflare', r.srflx.length ? 'PASS' : 'FAIL', r); } catch (e) { add('S8', 'safari srflx', 'FAIL', String(e)); }
    for (const b of cfg.snowflake.brokers) {
      let r = null;
      for (let i = 1; i <= 3 && !(r && r.ok); i++) { r = await C.snowflake(b, cfg.snowflake.fp, cfg.snowflake.stun, 20000); r.attempt = i; }
      add('E2', 'safari via ' + new URL(b).host, r.ok ? 'PASS' : 'FAIL', r);
    }
    for (const g of cfg.gateways) { const r = await C.gwCar(g, cfg.cid); add('C-P1', 'safari CAR from ' + new URL(g).host, r.ok ? 'PASS' : 'FAIL', r); }
    const put = await C.ipnsPut(cfg.delegated, cfg.ipnsName, cfg.ipnsRecord);
    add('C-P4', 'safari PUT IPNS record', put.ok ? 'PASS' : 'FAIL', put);
    if (put.ok) for (const g of cfg.gateways) {
      let r = null;
      for (let i = 0; i < 4 && !(r && r.ok); i++) { if (i) await new Promise((z) => setTimeout(z, 15000)); r = await C.ipnsGet(g, cfg.ipnsName, cfg.ipnsRecord); }
      add('C-P4', 'safari read back via ' + new URL(g).host, r.ok ? 'PASS' : 'FAIL', r);
    }
  }
  await fetch('/result', { method: 'POST', body: JSON.stringify(res) });
  log('DONE — you can close this tab.');
})();`;

const pageJs = fs.readFileSync(path.join(HERE, 'lib', 'page.js'));
let done;
const finished = new Promise((r) => { done = r; });
const srv = http.createServer((q, r) => {
  if (q.url === '/page.js') { r.writeHead(200, { 'content-type': 'text/javascript' }); r.end(pageJs); return; }
  if (q.url === '/runner.js') { r.writeHead(200, { 'content-type': 'text/javascript' }); r.end(runner); return; }
  if (q.url === '/result' && q.method === 'POST') {
    let b = ''; q.on('data', (d) => { b += d; }); q.on('end', () => { r.end('ok'); done(JSON.parse(b)); }); return;
  }
  r.writeHead(200, { 'content-type': 'text/html' });
  r.end('<!doctype html><meta charset=utf-8><title>p2p-chat Safari checks</title><h3>p2p-chat checks running in Safari…</h3><pre id=log></pre><script src="/page.js"></script><script src="/runner.js"></script>');
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const target = `http://127.0.0.1:${srv.address().port}/`;
console.log(`Opening ${target} in Safari (if nothing opens, paste the URL into Safari yourself)`);
if (process.platform === 'darwin') cp.spawn('open', ['-a', 'Safari', target], { stdio: 'ignore' });

const timeout = new Promise((r) => setTimeout(() => r(null), 10 * 60 * 1000));
const results = await Promise.race([finished, timeout]);
srv.close();
if (!results) { console.log('Safari checks timed out after 10 minutes'); process.exit(1); }
for (const x of results) console.log(`[${x.status}] ${x.id} ${x.name}: ${typeof x.details === 'string' ? x.details : JSON.stringify(x.details)}`);
fs.writeFileSync(path.join(OUT, 'safari.json'), JSON.stringify(results, null, 1));
const md = ['| ID | Check | Result | Details |', '|---|---|---|---|',
  ...results.map((r) => `| ${r.id} | ${r.name} | ${r.status} | ${(typeof r.details === 'string' ? r.details : JSON.stringify(r.details)).replace(/\|/g, '\\|').slice(0, 400)} |`)];
fs.writeFileSync(path.join(OUT, 'safari.md'), md.join('\n') + '\n');
process.exit(0);
