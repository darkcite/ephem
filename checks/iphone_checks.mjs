// iPhone checkpoints without GitHub Pages (works while the repo is private).
// Serves checks/web/ from this laptop through a free Cloudflare quick tunnel
// (`cloudflared`, no account), prints a QR code, and collects the results the iPhone sends.
// Usage: node iphone_checks.mjs <out-dir>     Env: S6_SECONDS (default 120)
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as url from 'node:url';
import * as cp from 'node:child_process';
import * as qrcode from 'qrcode';
import * as mk from './make_web_config.mjs';
import * as sv from './serve.mjs';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const OUT = process.argv[2] || path.join(HERE, 'out', 'iphone');
fs.mkdirSync(OUT, { recursive: true });

const cfg = await mk.baseConfig({
  label: 'iphone', net: true, autorun: false, interactive: true, cross: false, resultUrl: 'result',
  s6Seconds: Number(process.env.S6_SECONDS || 120),
});
let latest = [];
const { srv, port, finished } = await sv.startServer(cfg, { onPartial: (r) => { latest = r; console.log(`  … ${r.length} results received so far`); } });

// Cloudflare quick tunnel: prints https://<random>.trycloudflare.com on stderr.
const tunnel = cp.spawn('cloudflared', ['tunnel', '--no-autoupdate', '--url', `http://127.0.0.1:${port}`], { stdio: ['ignore', 'pipe', 'pipe'] });
tunnel.on('error', () => { console.log('cloudflared not found. Install it: brew install cloudflared'); process.exit(2); });
const publicUrl = await new Promise((res, rej) => {
  const t = setTimeout(() => rej(new Error('no tunnel URL after 60 s')), 60000);
  const scan = (d) => { const m = String(d).match(/https:\/\/[a-z0-9-]+\.trycloudflare\.com/); if (m) { clearTimeout(t); res(m[0]); } };
  tunnel.stdout.on('data', scan); tunnel.stderr.on('data', scan);
});
console.log(`\nOpen this on the iPhone (Safari), or scan the QR code with the Camera app:\n\n  ${publicUrl}/\n`);
console.log(await qrcode.toString(`${publicUrl}/`, { type: 'terminal', small: true }));
console.log(`Then tap 1 (checks), 2 (S6: stay away at least ${cfg.s6Seconds} s), 3 (S4 camera), 4 (send). Waiting up to 30 minutes…`);

const timeout = new Promise((r) => setTimeout(() => r(null), 30 * 60 * 1000));
const final = await Promise.race([finished, timeout]);
tunnel.kill(); srv.close();
const results = final || latest;
if (!results.length) { console.log('No results received.'); process.exit(1); }
if (!final) console.log('Timed out; writing the partial results received.');
sv.writeReport(OUT, 'iphone', results);
process.exit(0);
