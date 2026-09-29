// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Serves the Ephem app from this laptop to your phones and other devices through a free
// Cloudflare quick tunnel (`brew install cloudflared`, no account). The repo can stay private.
//
//   node checks/serve_app.mjs
//
// Prints https://<random>.trycloudflare.com/app/ and its QR code. Open it on both devices (or
// one device plus this laptop) and chat as usual: invite/answer links use that address.
// Only the static files pass through the tunnel; the chat itself is direct WebRTC between the
// devices, exactly as it will be on GitHub Pages. The address changes on every run: the service
// worker and saved state are per address. Ctrl+C stops it.
import * as cp from 'node:child_process';
import * as fs from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as url from 'node:url';
import QRCode from 'qrcode';

const ROOT = path.resolve(path.dirname(url.fileURLToPath(import.meta.url)), '..');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.md': 'text/plain; charset=utf-8' };

const srv = http.createServer((q, r) => {
  let p = decodeURIComponent(new URL(q.url, 'http://x').pathname);
  if (p.endsWith('/')) p += 'index.html';
  const f = path.join(ROOT, path.normalize(p));
  // Serve only the site (landing page, app, docs); never the rest of the repository.
  const allowed = f.startsWith(path.join(ROOT, 'app') + path.sep) || f.startsWith(path.join(ROOT, 'docs') + path.sep) ||
    ['index.html', 'site.css'].includes(path.relative(ROOT, f));
  if (!allowed || !fs.existsSync(f) || !fs.statSync(f).isFile()) { r.writeHead(404); r.end(); return; }
  r.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream', 'cache-control': 'no-cache' });
  fs.createReadStream(f).pipe(r);
});
await new Promise((res) => srv.listen(0, '127.0.0.1', res));
const port = srv.address().port;

const tunnel = cp.spawn('cloudflared', ['tunnel', '--no-autoupdate', '--url', `http://127.0.0.1:${port}`], { stdio: ['ignore', 'pipe', 'pipe'] });
tunnel.on('error', () => { console.log('cloudflared not found. Install it: brew install cloudflared'); process.exit(2); });
const base = await new Promise((res, rej) => {
  const t = setTimeout(() => rej(new Error('no tunnel address after 60 s')), 60000);
  const scan = (d) => { const m = String(d).match(/https:\/\/[a-z0-9-]+\.trycloudflare\.com/); if (m) { clearTimeout(t); res(m[0]); } };
  tunnel.stdout.on('data', scan);
  tunnel.stderr.on('data', scan);
});
const app = `${base}/app/`;
console.log(`\nEphem is served at:\n\n  ${app}\n\n(landing page: ${base}/)\n`);
console.log(await QRCode.toString(app, { type: 'terminal', small: true }));
console.log('Scan with each phone camera (or open the link). The tunnel may need ~20 s before it answers.');
console.log('Ctrl+C to stop.\n');

const stop = () => { tunnel.kill(); srv.close(); process.exit(0); };
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
