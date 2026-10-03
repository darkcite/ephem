// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// The service worker runs only the build the user accepted (docs/P2P-CHAT.md §17.1, security
// audit W-1): a new build is offered, survives closing and reopening the app without taking
// over, and runs only after "Update". (A hostile sw.js from the host is out of reach of any
// service worker: §5.)
import { check, finish, launch, serve, watch } from './e2e_lib.mjs';

let v2 = false;
const srv = await serve((p, text) => {
  if (!v2) return null;
  if (p === '/app/sw.js') return text().replace(/const VERSION = '[^']+'/, "const VERSION = 'v2test'");
  if (p === '/app/index.html' || p === '/app/') return text().replace('<title>Ephem</title>', '<title>Ephem v2</title>');
  return null;
});
const base = `http://127.0.0.1:${srv.address().port}/app/`;
const browser = await launch();
const title = (p) => p.evaluate(() => document.title);
const banner = (p) => p.waitForFunction(() => !document.getElementById('update').hidden, null, { timeout: 15000 }).then(() => true, () => false);

try {
  const ctx = await browser.newContext();
  let p = await ctx.newPage();
  watch(p, 'app');
  await p.goto(base);
  await p.evaluate(() => navigator.serviceWorker.ready);
  await p.reload();
  await p.waitForFunction(() => !!navigator.serviceWorker.controller);
  check('first install: build 1 runs, no update offered', (await title(p)) === 'Ephem' && !(await p.evaluate(() => !document.getElementById('update').hidden)));
  // W-3: the Tor pages belong to the installed build (precached), only the Tor wasm is lazy.
  const pages = await p.evaluate(async () => {
    const names = (await caches.keys()).filter((k) => k !== 'ephem-meta');
    const c = await caches.open(names[0]);
    return { tor: !!(await c.match('tor.html')), channel: !!(await c.match('channel.html')), wasm: !!(await c.match('pkg/ephem_tor_bg.wasm')) };
  });
  check('tor.html and channel.html precached with the build; the Tor wasm is not', pages.tor && pages.channel && !pages.wasm, JSON.stringify(pages));

  v2 = true;
  await p.evaluate(async () => (await navigator.serviceWorker.getRegistration()).update());
  check('a new build is offered while the app is open', await banner(p));
  check('…and the offer names the new build', /v2test/.test(await p.textContent('#update-text')), await p.textContent('#update-text'));

  await p.close();
  await new Promise((r) => setTimeout(r, 1500));
  p = await ctx.newPage();
  watch(p, 'app');
  await p.goto(base);
  check('closed and reopened without "Update": still build 1', (await title(p)) === 'Ephem', await title(p));
  check('…and the new build is offered again', await banner(p));

  await Promise.all([p.waitForEvent('load', { timeout: 20000 }), p.click('#b-update')]);
  await p.waitForFunction(() => document.title === 'Ephem v2', null, { timeout: 15000 }).catch(() => {});
  check('after "Update": build 2 runs', (await title(p)) === 'Ephem v2', await title(p));
  await p.reload();
  check('…and stays after a reload, with no offer', (await title(p)) === 'Ephem v2' && !(await p.evaluate(() => !document.getElementById('update').hidden)));
} catch (e) {
  check('service-worker update flow', false, e.message.split('\n')[0]);
} finally {
  await browser.close();
  srv.close();
}
finish();
