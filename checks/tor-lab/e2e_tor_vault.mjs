// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// One identity on several devices (docs/P2P-CHAT.md §D.11, V-2/V-3) in the offline lab. Three
// browsers stand for three devices of one saved identity; the vault goes through the lab's
// stand-in routing host (as delegated-ipfs.dev, through a Tor exit):
//
//   A creates a channel and posts → the vault lists it, with A's lease → B signs in with the
//   key file: the channel is listed as written by the other device → B takes over: it reads the
//   newest version from the channel's onion (A still serves it) and serves it itself; A notices
//   at its next renewal and stops → B posts → A and B close → C signs in: nobody holds the
//   channel's blocks, so once B's lease has run out C continues the channel without its older
//   posts: the sequence continues, the view says what is missing, and a new post is served.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`. Lab only (the stand-in routing host).
import * as fs from 'node:fs';
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, routingStandIn, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

const LEASE_S = 30;
const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const { server: routing, cfg, routed } = await routingStandIn();
const lab = { routing: cfg, leaseS: LEASE_S, restoreMs: 40_000 };
const browsers = [];

async function device(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }), lab);
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept());
  await p.goto(`${base}/tor.html`);
  await torReady(p, who);
  return { b, p };
}

/** Signs in with the saved identity's key file (Settings → Your identity → Sign in). */
async function signIn(p, file) {
  await toSettings(p);
  await p.click('#b-id-load');
  await p.setInputFiles('#i-file', { name: 'channels.p2pkey', mimeType: 'application/octet-stream', buffer: file });
  await p.fill('#i-pass-in', PASS);
  await p.click('#b-id-do-load');
  await p.waitForFunction(() => /Vault test/.test(document.querySelector('#id-desc')?.textContent), null, { timeout: 30_000 });
}

const view = (p) => p.evaluate(() => JSON.parse(globalThis.ephemChannel.view(0) || '{}'));
const ownRow = (p) => p.locator('#owns li').first();

try {
  // ---- device A: a saved identity, a channel, a post ----
  const A = await device('A');
  await toSettings(A.p);
  await A.p.click('#b-id-save');
  await A.p.fill('#i-label', 'Vault test');
  await A.p.fill('#i-pass', PASS);
  await A.p.fill('#i-pass2', PASS);
  const [dl] = await Promise.all([A.p.waitForEvent('download'), A.p.click('#b-id-do-save')]);
  const keyFile = fs.readFileSync(await dl.path());
  await A.p.click('#tab-own');
  await A.p.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  await A.p.fill('#i-title', 'Multi');
  await A.p.fill('#i-about', 'One identity, several devices');
  await A.p.check('#c-understood');
  await A.p.click('#b-create');
  await A.p.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  await A.p.fill('#t-post', 'from A');
  await A.p.click('#b-post');
  await A.p.waitForFunction(() => { const v = globalThis.ephemChannel.vault(); return v && JSON.parse(v).channels.some((c) => c.count === 1); }, null, { timeout: 60_000 });
  const vA = JSON.parse(await A.p.evaluate(() => globalThis.ephemChannel.vault()));
  check('A publishes the vault through a Tor exit: the channel, with A\'s lease', vA.channels[0].title === 'Multi' && vA.until * 1000 > Date.now(), `vault sequence ${vA.seq}, ${routed.filter((q) => q.method === 'PUT').length} PUTs`);
  const vaultPut = routed.find((q) => q.method === 'PUT');
  check('the vault record is opaque: no title or post text in it', !vaultPut.body.includes(Buffer.from('Multi')) && vaultPut.body.length <= 10240, `${vaultPut.body.length} bytes`);

  // ---- device B: signs in; the channel belongs to the other device's lease ----
  const B = await device('B');
  await signIn(B.p, keyFile);
  await B.p.click('#tab-own');
  await B.p.waitForFunction(() => /written by your other device/.test(document.querySelector('#owns li')?.textContent), null, { timeout: T });
  check('B lists the channel as written by the other device (no blocks needed)', /Multi/.test(await ownRow(B.p).textContent()));
  await ownRow(B.p).click();
  await B.p.waitForSelector('#v-own-away:not([hidden])');
  const t1 = Date.now();
  await B.p.click('#b-takeover');
  await B.p.waitForFunction(() => !document.querySelector('#v-own').hidden && /from A/.test(document.querySelector('#o-posts')?.textContent) && /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  check('B takes over: the newest version read from the channel onion, now served by B', (await view(B.p)).missing === 0, `${Date.now() - t1} ms`);
  const t2 = Date.now();
  await A.p.waitForFunction(() => /took over your channels/.test(document.querySelector('#error')?.textContent), null, { timeout: LEASE_S * 1000 + 60_000 });
  check('A notices at its next renewal and stops writing', /written by your other device/.test(await ownRow(A.p).textContent()), `${Date.now() - t2} ms`);
  await B.p.fill('#t-post', 'from B');
  await B.p.click('#b-post');
  await B.p.waitForFunction(() => { const v = globalThis.ephemChannel.vault(); return v && JSON.parse(v).channels[0].count === 2; }, null, { timeout: 60_000 });
  const vB = JSON.parse(await B.p.evaluate(() => globalThis.ephemChannel.vault()));
  const seqB = (await view(B.p)).sequence;
  await A.b.close();
  await B.b.close();

  // ---- device C: no device online, no mirror: continue without the older posts ----
  const C = await device('C');
  await signIn(C.p, keyFile);
  await C.p.click('#tab-own');
  const wait = Math.max(0, vB.until * 1000 - Date.now());
  const t3 = Date.now();
  await C.p.waitForFunction(() => document.querySelector('#owns li') && !/other device/.test(document.querySelector('#owns li').textContent), null, { timeout: wait + T });
  await ownRow(C.p).click();
  await C.p.waitForFunction(() => !document.querySelector('#v-own').hidden && !document.querySelector('#o-missing').hidden, null, { timeout: T });
  const vc = await view(C.p);
  check('C (the lease ran out, no host has the blocks) continues the channel without its older posts',
    vc.missing === 2 && vc.posts.length === 0 && vc.sequence > seqB && /2 older posts/.test(await C.p.textContent('#o-missing')),
    `after ${Date.now() - t3} ms (lease wait ${Math.round(wait / 1000)} s); missing ${vc.missing}, sequence ${seqB} → ${vc.sequence}`);
  await C.p.fill('#t-post', 'from C');
  await C.p.click('#b-post');
  await C.p.waitForFunction(() => /from C/.test(document.querySelector('#o-posts')?.textContent));
  const vc2 = await view(C.p);
  check('C\'s post continues the same chain (seq 3 on top of the 2 missing ones)', vc2.posts[0]?.seq === 3 && vc2.missing === 2);
  await C.p.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('vault flow', false, e.message.split('\n')[0]);
  dumpLogs(80);
} finally {
  for (const b of browsers) await b.close().catch(() => {});
  srv.close();
  routing.close();
}
finish();
