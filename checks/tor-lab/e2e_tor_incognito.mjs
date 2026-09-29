// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// The owner's report, step by step (docs/P2P-CHAT.md §D.11): a channel created with a saved
// identity on one device ("the laptop"), then the same identity signed in in a fresh browser
// profile ("incognito"): the channel must show in My channels. MODE=tor (tor.html, default) or
// MODE=direct (/app/: channels need their own sign-in there). Prints what the incognito page
// shows every 10 s. Lab, or RELAY=1 for the real Tor network and the real delegated-ipfs.dev.
import * as fs from 'node:fs';
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { REAL, T, dumpLogs, record, routingStandIn, serveTor, torContext, unexpected } from './tor_env.mjs';

const MODE = process.env.MODE || 'tor';
const page = MODE === 'tor' ? 'tor.html' : '';
const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const { server: routing, cfg } = await routingStandIn();
const browsers = [];
const lines = [];

async function device(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }), REAL ? {} : { routing: cfg });
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('console', (m) => { if (/vault|channel/i.test(m.text())) lines.push(`${who} | ${m.text().slice(0, 200)}`); });
  p.on('dialog', (d) => d.accept());
  await p.goto(`${base}/${page}`);
  return p;
}

/** Direct mode: My channels asks for the passphrase once (its own sign-in). */
async function channelSignIn(p, file) {
  await p.waitForSelector('#signin:not([hidden])', { timeout: T });
  await p.setInputFiles('#i-ch-file', { name: 'key.p2pkey', mimeType: 'application/octet-stream', buffer: file });
  await p.fill('#i-ch-pass', PASS);
  await p.click('#b-signin');
}

const state = (p) => p.evaluate(() => ({
  owns: document.querySelector('#owns')?.textContent.trim(),
  sync: document.querySelector('#own-sync:not([hidden])')?.textContent,
  signin: !document.querySelector('#signin')?.hidden,
  pane: [...document.querySelectorAll('#pane > .view:not([hidden])')].map((v) => v.id).join(),
  chState: document.querySelector('#ch-state')?.textContent,
  error: document.querySelector('#error:not([hidden])')?.textContent,
  diag: document.querySelector('#own-diag-text')?.textContent,
}));

try {
  // ---- the laptop ----
  const a = await device('laptop');
  await toSettings(a);
  await a.click('#b-id-save');
  await a.fill('#i-label', 'Incognito test');
  await a.fill('#i-pass', PASS);
  await a.fill('#i-pass2', PASS);
  const [dl] = await Promise.all([a.waitForEvent('download'), a.click('#b-id-do-save')]);
  const key = fs.readFileSync(await dl.path());
  await a.click('#tab-own');
  if (MODE === 'direct') await channelSignIn(a, key);
  await a.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: T });
  await a.fill('#i-title', 'Laptop channel');
  await a.check('#c-understood');
  await a.click('#b-create');
  await a.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  await a.waitForFunction(() => { try { const v = globalThis.ephemChannel.vault(); return v && JSON.parse(v).channels.length > 0; } catch { return false; } }, null, { timeout: T })
    .then(() => check('laptop: channel online, vault published', true), () => check('laptop: channel online, vault published', false, 'no vault after the timeout'));

  // ---- incognito: the same identity, a fresh profile ----
  const b = await device('incognito');
  await toSettings(b);
  await b.click('#b-id-load');
  await b.setInputFiles('#i-file', { name: 'key.p2pkey', mimeType: 'application/octet-stream', buffer: key });
  await b.fill('#i-pass-in', PASS);
  await b.click('#b-id-do-load');
  await b.waitForFunction(() => /Incognito test/.test(document.querySelector('#id-desc')?.textContent), null, { timeout: 30_000 });
  await b.click('#tab-own');
  if (MODE === 'direct') await channelSignIn(b, key);
  const t0 = Date.now();
  let seen = false;
  while (Date.now() - t0 < T && !seen) {
    await b.waitForTimeout(10_000);
    const s = await state(b);
    console.log(`  +${Math.round((Date.now() - t0) / 1000)} s incognito:`, JSON.stringify(s));
    seen = /Laptop channel/.test(s.owns || '');
  }
  check(`incognito (${MODE} mode): the laptop's channel is listed`, seen, `${Math.round((Date.now() - t0) / 1000)} s`);
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('incognito flow', false, e.message.split('\n')[0]);
  dumpLogs(40);
} finally {
  console.log(lines.slice(-30).join('\n'));
  for (const b of browsers) await b.close().catch(() => {});
  srv.close();
  routing.close();
}
finish();
