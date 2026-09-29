// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Public channels (docs/P2P-CHAT.md §27, Appendix D and F.3; CH-2…CH-5, UI-3…UI-5) in the
// offline lab, in the app's Following and My channels tabs:
//
//   owner (Tor mode, saved chat identity) creates a channel in My channels (warnings
//   acknowledged; keys derived from the identity), posts 3, deletes 1 → served on its own onion
//   from the tab, sharing the chats' Tor client → a second channel, both online → a reader in
//   another browser opens the link, verifies it over Tor and follows it → a new post reaches the
//   follow list → the reader mirrors it (a second onion) → the owner signs the mirror into the
//   manifest → C-P3 (1 000 posts) → IPNS publish through a Tor exit → the owner's channels come
//   back after a reload → backup export → a direct-mode page loads the Tor build only for its
//   channel tab and reads the channel → the owner closes the tab → a new reader reads the
//   channel from the mirror.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 for the real Tor network.
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as https from 'node:https';
import * as os from 'node:os';
import * as path from 'node:path';
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { LIVE, T, dumpLogs, record, serveTor, torBrowserGet, torContext, unexpected } from './tor_env.mjs';

// A stand-in public IPFS gateway (§D.6.2) for the no-Tor reader: it serves what a follower's
// Kubo mirror would (the CAR and record the reader downloads with "For IPFS (Kubo)").
const gw = { record: null, car: null };
const srv = await serveTor((u) => {
  if (u.pathname.startsWith('/gw/ipns/') && gw.record) return { type: 'application/vnd.ipfs.ipns-record', body: gw.record };
  if (u.pathname.startsWith('/gw/ipfs/') && gw.car) return { type: 'application/vnd.ipld.car', body: gw.car };
  if (u.pathname.startsWith('/gw/')) return { status: 404, type: 'text/plain', body: 'not found' };
  return null;
});
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];

// A stand-in for delegated-ipfs.dev (§D.5.2): HTTPS on 127.0.0.1 with a throwaway CA, reached
// by the owner's page through a lab exit relay. It keeps what was PUT.
const certs = fs.mkdtempSync(path.join(os.tmpdir(), 'ephem-ca-'));
const ssl = (...a) => execFileSync('openssl', a, { cwd: certs, stdio: 'ignore' });
ssl('req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes', '-days', '2', '-subj', '/CN=Ephem lab CA', '-keyout', 'ca.key', '-out', 'ca.pem', '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign');
ssl('req', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes', '-subj', '/CN=127.0.0.1', '-keyout', 'leaf.key', '-out', 'leaf.csr');
fs.writeFileSync(path.join(certs, 'ext'), 'subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n');
ssl('x509', '-req', '-in', 'leaf.csr', '-CA', 'ca.pem', '-CAkey', 'ca.key', '-CAcreateserial', '-days', '2', '-extfile', 'ext', '-out', 'leaf.pem');
ssl('x509', '-in', 'ca.pem', '-outform', 'DER', '-out', 'ca.der');
const routed = [];
const routing = https.createServer({ key: fs.readFileSync(path.join(certs, 'leaf.key')), cert: fs.readFileSync(path.join(certs, 'leaf.pem')) }, (q, res) => {
  const body = [];
  q.on('data', (c) => body.push(c));
  q.on('end', () => {
    routed.push({ method: q.method, url: q.url, type: q.headers['content-type'], body: Buffer.concat(body) });
    res.writeHead(200);
    res.end();
  });
});
await new Promise((ok) => routing.listen(0, '127.0.0.1', ok));
const routingCfg = { host: `127.0.0.1:${routing.address().port}`, root: fs.readFileSync(path.join(certs, 'ca.der')).toString('base64') };

async function page(who, extra = {}) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }), extra);
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept());
  return p;
}

const posts = (p) => p.$$eval('#r-posts li .body, #o-posts li .body', (ls) => ls.map((l) => l.textContent));
const tab = (p, t) => p.click(`#tab-${t}`);

async function newChannel(o, title, about) {
  await o.click('#b-own-new');
  await o.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  await o.fill('#i-title', title);
  await o.fill('#i-about', about);
  await o.check('#c-understood');
  await o.click('#b-create');
  await o.waitForFunction((t) => document.querySelector('#o-title')?.textContent === t && !document.querySelector('#v-own').hidden, title);
}

try {
  // ---- owner: Tor mode, a saved identity (channels are owned by the chat identity, D3) ----
  const o = await page('owner', { routing: routingCfg });
  await o.goto(`${base}/tor.html`);
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.click('#b-id-save');
  await o.fill('#i-label', 'Channels');
  await o.fill('#i-pass', PASS);
  await o.fill('#i-pass2', PASS);
  await Promise.all([o.waitForEvent('download'), o.click('#b-id-do-save')]);
  await tab(o, 'own');
  await o.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  check('My channels: owned by the chat identity, no second sign-in', /Owned by your identity “Channels”/.test(await o.textContent('#ch-identity')) && await o.isHidden('#signin'));
  await o.fill('#i-title', 'Lab news');
  await o.fill('#i-about', 'Posts from the offline Tor lab');
  await o.click('#b-create');
  check('the warnings must be acknowledged', /understand/.test(await o.textContent('#error')));
  await o.check('#c-understood');
  await o.click('#b-create');
  await o.waitForSelector('#v-own:not([hidden])');
  for (const t of ['first post', 'second post', 'third post']) {
    await o.fill('#t-post', t);
    await o.click('#b-post');
    await o.waitForFunction((t) => [...document.querySelectorAll('#o-posts .body')].some((b) => b.textContent === t), t);
  }
  await o.locator('#o-posts li', { hasText: 'second post' }).locator('button', { hasText: 'Delete' }).click();
  await o.waitForFunction(() => /deleted by the owner/.test(document.querySelector('#o-posts')?.textContent));
  const t0 = Date.now();
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  const link = await o.inputValue('#o-link');
  check('channel created, 3 posts, 1 deleted, online on its own onion', /\/tor\.html#c=k51.*&o=[a-z2-7]{56}\.onion$/.test(link), `${Date.now() - t0} ms to online`);

  // Without Ephem: the onion's root is a plain page for Tor Browser (no scripts), fetched here
  // through the lab's C Tor client.
  if (!LIVE) {
    const onion = link.match(/&o=([a-z2-7]{56}\.onion)/)[1];
    const page = await torBrowserGet(onion, '/');
    check('Tor Browser: the owner\'s onion serves the channel as a plain page (no scripts)',
      page.status === 200 && /Content-Type: text\/html/.test(page.headers) && /Content-Security-Policy: default-src 'none'/.test(page.headers)
        && page.body.includes('Lab news') && page.body.includes('third post') && page.body.includes('deleted by the owner')
        && page.body.includes("the channel's own onion address") && !/<script/i.test(page.body) && (await o.textContent('#o-plain-url')) === `http://${onion}/`,
      `${page.body.length} B`);
  }

  // UI-5: a second channel, both online at once.
  await newChannel(o, 'Lab two', 'A second channel');
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  const link2 = await o.inputValue('#o-link');
  await o.waitForFunction(() => document.querySelectorAll('#owns li.ok').length === 2, null, { timeout: T });
  check('two channels owned, both online (their own onions)', link2 !== link && (await o.$$eval('#owns li b', (b) => b.map((x) => x.textContent))).join('|') === 'Lab news|Lab two');
  await o.locator('#owns li', { hasText: 'Lab news' }).click();
  await o.waitForFunction(() => document.querySelector('#o-title')?.textContent === 'Lab news');

  // ---- reader over Tor: opens the link, follows ----
  const r = await page('reader');
  const t1 = Date.now();
  await r.goto(link);
  await r.waitForFunction(() => /Verified/.test(document.querySelector('#r-source')?.textContent), null, { timeout: T });
  const got = await posts(r);
  check('a reader verifies the channel over Tor (newest first, deleted shown as such)',
    (await r.textContent('#r-title')) === 'Lab news' && got[0] === 'third post' && /deleted by the owner/.test(got[1]) && got[2] === 'first post', `${Date.now() - t1} ms`);
  await r.click('#b-follow');
  await r.waitForFunction(() => /Lab news/.test(document.querySelector('#follows')?.textContent));
  check('Follow: the channel is in the Following list', await r.isVisible('#b-unfollow'));

  // A new post reaches the follow list (the tab refreshes the channels it lists).
  await o.fill('#t-post', 'fourth post');
  await o.click('#b-post');
  await o.waitForFunction(() => [...document.querySelectorAll('#o-posts .body')].some((b) => b.textContent === 'fourth post'));
  await tab(r, 'chats');
  await tab(r, 'follow');
  await r.waitForFunction(() => [...document.querySelectorAll('#r-posts .body')].some((b) => b.textContent === 'fourth post'), null, { timeout: T });
  check('a followed channel refreshes: the new post arrives', true);

  // The second channel reads the same way.
  await r.goto(link2.replace('/tor.html#', '/tor.html?2#'));
  await r.waitForFunction(() => document.querySelector('#r-title')?.textContent === 'Lab two', null, { timeout: T });
  check('the second channel is readable too', /Verified/.test(await r.textContent('#r-source')));
  await r.goto(link.replace('/tor.html#', '/tor.html?1#'));
  await r.waitForFunction(() => document.querySelector('#r-title')?.textContent === 'Lab news', null, { timeout: T });

  // ---- mirror ----
  await r.click('#b-mirror');
  await r.waitForSelector('#mirror-note:not([hidden])', { timeout: 60_000 });
  const mirror = (await r.textContent('#mirror-text')).match(/[a-z2-7]{56}\.onion/)[0];
  check('the reader mirrors it on a second onion of its tab', !!mirror, mirror);
  if (!LIVE) {
    const mp = await torBrowserGet(mirror, '/');
    check('Tor Browser: the mirror serves the page too, saying it is a mirror', mp.status === 200 && mp.body.includes('Served by a mirror') && mp.body.includes('fourth post'));
  }
  await o.fill('#i-mirrors', mirror);
  await o.click('#b-mirrors');
  await o.waitForFunction((m) => document.querySelector('#o-link')?.value.includes(m), mirror);
  const withMirror = await o.inputValue('#o-link');
  await r.click('#b-refresh');
  await r.waitForFunction(() => /version 7/.test(document.querySelector('#r-source')?.textContent), null, { timeout: T });
  check('owner signs the mirror list; the reader sees the new version (and mirrors it)', true, await r.textContent('#r-source'));

  // C-P3 and IPNS publishing drive the page through the lab hook and the lab's stand-in
  // routing host: lab only.
  if (!LIVE) {
    // ---- C-P3: a channel of 1 000 posts over an onion ----
    const t3 = Date.now();
    await o.evaluate(() => { for (let i = 1; i <= 1000; i++) globalThis.ephemChannel.post(0, `bulk post ${i}`, 0); });
    const ownerMs = Date.now() - t3;
    const size = await o.evaluate(() => globalThis.ephemChannel.car(0).length);
    const t4 = Date.now();
    await r.click('#b-refresh');
    await r.waitForFunction(() => document.querySelectorAll('#r-posts li').length >= 1004, null, { timeout: T });
    check('C-P3: 1 000 more posts; a reader fetches and verifies them over the onion', true,
      `owner ${ownerMs} ms for 1 000 posts, CAR ${Math.round(size / 1024)} KB; reader ${Date.now() - t4} ms (fetch + verify + render)`);

    // ---- optional IPNS publishing through a Tor exit (§D.5.2) ----
    await o.click('#b-publish');
    await o.waitForFunction(() => /published|failed/.test(document.querySelector('#publish-state')?.textContent), null, { timeout: T });
    const put = routed[0];
    const ownRecord = Buffer.from(await o.evaluate(() => Array.from(globalThis.ephemChannel.record(0))));
    check('owner publishes the IPNS record through a Tor exit (HTTPS PUT, routing API)',
      /published/.test(await o.textContent('#publish-state')) && put?.method === 'PUT' && put.url === `/routing/v1/ipns/${link.match(/#c=([^&]+)/)[1]}` && put.type === 'application/vnd.ipfs.ipns-record' && put.body.equals(ownRecord),
      await o.textContent('#publish-state'));

  }

  // ---- the owner's channels come back after a reload; the backup exports ----
  await o.reload();
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.locator('#slots li', { hasText: 'Channels' }).locator('button', { hasText: 'Sign in' }).click();
  await o.locator('#slots li input[type=password]').fill(PASS);
  await o.locator('#slots li', { hasText: 'Channels' }).locator('button', { hasText: 'Sign in' }).click();
  await o.waitForFunction(() => /Channels/.test(document.querySelector('#id-desc')?.textContent));
  await tab(o, 'own');
  await o.waitForFunction(() => document.querySelectorAll('#owns li').length === 2, null, { timeout: 30_000 });
  await o.locator('#owns li', { hasText: 'Lab news' }).click();
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  check('after a reload both channels are back from the store and online again', (await posts(o)).includes('third post') && (await o.inputValue('#o-link')).includes(mirror));
  const [d1] = await Promise.all([o.waitForEvent('download'), o.click('#b-export')]);
  check('backup exports as channel.car (+ record)', d1.suggestedFilename() === 'channel.car');

  // ---- no Tor: a public gateway serving a follower's IPFS mirror (the Kubo downloads) ----
  await r.click('#b-kubo');
  const [dc] = await Promise.all([r.waitForEvent('download'), r.click('#b-dl-car')]);
  const [dr] = await Promise.all([r.waitForEvent('download'), r.click('#b-dl-record')]);
  gw.car = fs.readFileSync(await dc.path());
  gw.record = fs.readFileSync(await dr.path());
  check('Kubo mirror instructions and downloads', /ipfs dag import channel\.car/.test(await r.textContent('#kubo-cmds')) && gw.car.length > 1000);
  // The gateway stand-in is the lab's (LIVE would need a real IPFS mirror of this channel).
  if (!LIVE) {
    const g = await page('gateway-reader', { gateway: `${base.replace('/app', '')}/gw` });
    await g.goto(link.replace(/&o=.*$/, ''));
    await g.waitForSelector('#gateway-warn:not([hidden])', { timeout: 30_000 });
    await g.click('#b-gateway-go');
    await g.waitForFunction(() => /through the public gateway/.test(document.querySelector('#r-source')?.textContent), null, { timeout: 30_000 });
    check('without Tor: read and verified through a public gateway (IP warning shown first)', (await posts(g)).includes('third post'));
  }

  // ---- direct mode: the Tor build only once a channel tab is used (UI-3) ----
  const d = await page('direct');
  const torLoads = [];
  d.on('request', (q) => { if (/ephem_tor/.test(q.url())) torLoads.push(q.url()); });
  await d.goto(`${base}/`);
  await d.waitForSelector('#v-start:not([hidden])');
  await d.waitForTimeout(1000);
  const before = torLoads.length;
  await tab(d, 'follow');
  await d.waitForSelector('#v-follow-new:not([hidden])', { timeout: 60_000 });
  await d.fill('#t-channel', link);
  await d.click('#b-channel-open');
  await d.waitForFunction(() => /Verified/.test(document.querySelector('#r-source')?.textContent), null, { timeout: T });
  check('direct mode: the Tor build loads only for the channel tab, then reads the channel', before === 0 && torLoads.length > 0 && (await d.textContent('#r-title')) === 'Lab news', `${torLoads.length} requests`);

  // ---- owner offline: a new reader gets it from the mirror ----
  await o.close();
  const n = await page('reader2');
  const t2 = Date.now();
  await n.goto(withMirror);
  await n.waitForFunction(() => /Verified/.test(document.querySelector('#r-source')?.textContent), null, { timeout: T });
  check('owner offline: a new reader reads the channel from the mirror (addresses tried in parallel)', (await posts(n)).includes('third post'), `${Date.now() - t2} ms`);
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('channel flow', false, e.message.split('\n')[0]);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
  routing.close();
  fs.rmSync(certs, { recursive: true });
}
finish();
