// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// The service worker runs only the build the user accepted (docs/P2P-CHAT.md §17.1, security
// audit W-1): a new build is offered, survives closing and reopening the app without taking
// over, and runs only after "Update". (A hostile sw.js from the host is out of reach of any
// service worker: §5.)
import { check, finish, launch, serve, watch } from './e2e_lib.mjs';

let v2 = false;
// Build 3 (a later deploy): a new worker and a Tor build the accepted pages' integrity no longer matches.
let v3 = false;
const srv = await serve((p, text) => {
  if (v3) {
    if (p === '/app/sw.js') return text().replace(/const VERSION = '[^']+'/, "const VERSION = 'v3test'");
    if (p === '/app/pkg/ephem_tor.js') return `${text()}\n// build 3\n`;
    return null;
  }
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
}

// The accepted build's Tor files are fetched on first use; after a later deploy the host serves
// another build's, whose integrity the accepted tor.html refuses. The page must still offer the
// update (not stay stuck), and a device that uses the Tor build gets it precached with each update.
v2 = false;
try {
  const ctx = await browser.newContext();
  let p = await ctx.newPage();
  watch(p, 'stuck');
  await p.goto(base);
  await p.evaluate(() => navigator.serviceWorker.ready);
  await p.reload();
  await p.waitForFunction(() => !!navigator.serviceWorker.controller);
  v3 = true;
  await p.goto(`${base}tor.html`);
  const stuck = await p.waitForFunction(() => /could not start/.test(document.body.textContent), null, { timeout: 20000 }).then(() => true, () => false);
  check('a Tor build from a later deploy fails the accepted pages\' integrity check', stuck);
  check('…and the failed start still offers the new build (Update)', await banner(p));
  check('…without switching to it on its own', await p.evaluate(async () => {
    const r = await (await caches.open('ephem-meta')).match(new URL('__accepted', (await navigator.serviceWorker.getRegistration()).scope).href);
    return (await r.text()) !== 'ephem-v3test';
  }));
  await p.close();
  v3 = false;

  // A device on the Tor build: the next update precaches that build's Tor files too.
  const ctx2 = await browser.newContext();
  p = await ctx2.newPage();
  watch(p, 'tor');
  await p.goto(`${base}tor.html`);
  await p.evaluate(() => navigator.serviceWorker.ready);
  await p.reload();
  await p.waitForFunction(async () => {
    for (const k of await caches.keys()) if (await (await caches.open(k)).match('pkg/ephem_tor.js')) return true;
    return false;
  }, null, { timeout: 30000, polling: 500 });
  v3 = true;
  await p.evaluate(async () => (await navigator.serviceWorker.getRegistration()).update());
  check('an update on a Tor device is offered', await banner(p));
  const pre = await p.evaluate(async () => {
    const c = await caches.open('ephem-v3test');
    return { js: !!(await c.match('pkg/ephem_tor.js')), wasm: !!(await c.match('pkg/ephem_tor_bg.wasm')) };
  });
  check('…with its own Tor build precached (pages and Tor files from one deploy)', pre.js && pre.wasm, JSON.stringify(pre));
} catch (e) {
  check('stuck start / Tor precache', false, e.message.split('\n')[0]);
} finally {
  v3 = false;
}
await browser.close();
srv.close();
finish();
