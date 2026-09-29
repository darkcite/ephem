// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Shared local server for real-browser checkpoint runs (safari_checks.mjs, iphone_checks.mjs):
// serves checks/web/, a per-run config.json, a mailbox for cross-engine field exchange,
// and collects posted results.
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as http from 'node:http';
import * as url from 'node:url';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const WEB = path.join(HERE, 'web');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json' };

export async function startServer(cfg, { host = '127.0.0.1', onPartial = () => {} } = {}) {
  const box = new Map(); const waiters = new Map();
  const mbPut = (k, v) => { box.set(k, v); (waiters.get(k) || []).forEach((w) => w(v)); waiters.delete(k); };
  const mbGet = (k, ms = 60000) => new Promise((res, rej) => {
    if (box.has(k)) return res(box.get(k));
    const t = setTimeout(() => rej(new Error(`mailbox timeout: ${k}`)), ms);
    waiters.set(k, [...(waiters.get(k) || []), (v) => { clearTimeout(t); res(v); }]);
  });
  let done; const finished = new Promise((r) => { done = r; });

  const srv = http.createServer((q, r) => {
    const u = new URL(q.url, 'http://x');
    const body = () => new Promise((res) => { let b = ''; q.on('data', (d) => { b += d; }); q.on('end', () => res(b)); });
    if (u.pathname === '/peer.html') { r.writeHead(200, { 'content-type': 'text/html' }); r.end('<!doctype html><meta charset=utf-8><script src="page.js"></script>'); return; }
    if (u.pathname === '/config.json') { r.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' }); r.end(JSON.stringify(cfg)); return; }
    if (u.pathname.startsWith('/mb/')) {
      const k = u.pathname.slice(4);
      if (q.method === 'POST') { body().then((b) => { mbPut(k, JSON.parse(b)); r.end('ok'); }); return; }
      mbGet(k, 25000).then((v) => { r.writeHead(200, { 'content-type': 'application/json' }); r.end(JSON.stringify(v)); })
        .catch(() => { r.writeHead(204); r.end(); });
      return;
    }
    if (u.pathname === '/result' && q.method === 'POST') {
      body().then((b) => { r.end('ok'); const res = JSON.parse(b); if (u.searchParams.get('final')) done(res); else onPartial(res); });
      return;
    }
    const f = path.join(WEB, u.pathname === '/' ? 'index.html' : path.normalize(u.pathname).replace(/^\/+/, ''));
    if (!f.startsWith(WEB) || !fs.existsSync(f)) { r.writeHead(404); r.end(); return; }
    r.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream', 'cache-control': 'no-store' });
    r.end(fs.readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, host, r));
  return { srv, port: srv.address().port, target: `http://127.0.0.1:${srv.address().port}/`, mbGet, mbPut, finished };
}

export function writeReport(outDir, name, results) {
  for (const x of results) console.log(`[${x.status}] ${x.id} ${x.name}: ${typeof x.details === 'string' ? x.details : JSON.stringify(x.details)}`);
  fs.writeFileSync(path.join(outDir, `${name}.json`), JSON.stringify(results, null, 1));
  const md = ['| ID | Check | Result | Details |', '|---|---|---|---|',
    ...results.map((r) => `| ${r.id} | ${r.name} | ${r.status} | ${(typeof r.details === 'string' ? r.details : JSON.stringify(r.details)).replace(/\|/g, '\\|').slice(0, 400)} |`)];
  fs.writeFileSync(path.join(outDir, `${name}.md`), md.join('\n') + '\n');
}
