// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// The online notice (docs/P2P-CHAT.md F.7) in the offline lab: a reader follows a channel and
// turns the setting on → the owner is online (no notice: unknown → online) → the owner reloads
// (offline) → two missed probes mark it offline in the follow list → the owner signs in again
// → the next probe finds it online: an in-app notice, and a tap opens the channel.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`. Probes run through the lab hook
// `ephemProbeAll` (the page's timer runs every 2 minutes).
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, serveTor, torContext, unexpected } from './tor_env.mjs';

const srv = await serveTor(() => null);
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];

async function page(who) {
  const b = await launch();
  browsers.push(b);
  const p = await (await torContext(await b.newContext({ acceptDownloads: true }))).newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept());
  return p;
}

const tab = (p, t) => p.click(`#tab-${t}`);
const row = (r) => r.textContent('#follows');
const probe = (r) => r.evaluate(() => globalThis.ephemProbeAll());

try {
  // ---- owner: a saved identity, one channel ----
  const o = await page('owner');
  await o.goto(`${base}/tor.html`);
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.click('#b-id-save');
  await o.fill('#i-label', 'Online');
  await o.fill('#i-pass', PASS);
  await o.fill('#i-pass2', PASS);
  await Promise.all([o.waitForEvent('download'), o.click('#b-id-do-save')]);
  await tab(o, 'own');
  await o.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  await o.fill('#i-title', 'Comes and goes');
  await o.fill('#i-about', 'An owner that goes offline');
  await o.check('#c-understood');
  await o.click('#b-create');
  await o.waitForSelector('#v-own:not([hidden])');
  await o.fill('#t-post', 'hello');
  await o.click('#b-post');
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  const link = await o.inputValue('#o-link');

  // ---- reader: follows, turns the setting on ----
  const r = await page('reader');
  await r.goto(link);
  await r.waitForFunction(() => /Verified/.test(document.querySelector('#r-source')?.textContent), null, { timeout: T });
  await r.click('#b-follow');
  await r.waitForFunction(() => /Comes and goes/.test(document.querySelector('#follows')?.textContent));
  await toSettings(r);
  check('the online notice is off by default', !(await r.isChecked('#c-online')));
  await r.check('#c-online');
  await tab(r, 'chats');
  await probe(r);
  check('owner online at the first probe: no notice (unknown → online is no news)',
    !/owner offline/.test(await row(r)) && (await r.locator('#notices .notice').count()) === 0);

  // ---- the owner goes offline (a reload: the tab's onion service stops until sign-in) ----
  await o.reload();
  await o.waitForSelector('#v-start:not([hidden])');
  const t0 = Date.now();
  await probe(r);
  const once = /owner offline/.test(await row(r));
  await probe(r);
  check('one missed probe is no outage; two in a row mark the owner offline', !once && /owner offline/.test(await row(r)), `${Date.now() - t0} ms for two probes`);
  check('no notice while it is offline', (await r.locator('#notices .notice').count()) === 0);

  // ---- the owner comes back ----
  await toSettings(o);
  await o.locator('#slots li', { hasText: 'Online' }).locator('button', { hasText: 'Sign in' }).click();
  await o.locator('#slots li input[type=password]').fill(PASS);
  await o.locator('#slots li', { hasText: 'Online' }).locator('button', { hasText: 'Sign in' }).click();
  await o.waitForFunction(() => /Online/.test(document.querySelector('#id-desc')?.textContent));
  await tab(o, 'own');
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving')?.textContent), null, { timeout: T });
  const t1 = Date.now();
  const note = r.locator('#notices .notice', { hasText: 'Comes and goes' });
  // A probe on the cached descriptor may miss the new introduction points: a few rounds.
  for (let i = 0; i < 4 && /owner offline/.test(await row(r)); i++) await probe(r);
  await note.waitFor({ timeout: 10_000 });
  const text = (await note.textContent()).trim();
  check('the owner is back: an in-app notice, the follow list no longer says offline',
    /Online again/.test(text) && !/owner offline/.test(await row(r)), `${text}; ${Date.now() - t1} ms`);
  await note.click();
  await r.waitForFunction(() => !document.querySelector('#v-read').hidden && document.querySelector('#r-title')?.textContent === 'Comes and goes', null, { timeout: T });
  check('a tap on the notice opens the channel', true);

  // ---- the setting off: no probes, no offline marks ----
  await toSettings(r);
  await r.uncheck('#c-online');
  check('turned off: the state is forgotten', !/owner offline/.test(await row(r)));
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('online flow', false, e.message.split('\n')[0]);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
