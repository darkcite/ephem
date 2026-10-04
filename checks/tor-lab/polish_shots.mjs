// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Screenshots of the board screens (desktop owner, phone reader) for a UI review. Not a test.
import { launch, PASS, toSettings } from '../e2e_lib.mjs';
import { serveTor, torContext, torReady } from './tor_env.mjs';

const OUT = process.env.SHOTS || '/tmp/shots';
const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];
async function page(w, h, mobile = false) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ viewport: { width: w, height: h }, isMobile: mobile, hasTouch: mobile, deviceScaleFactor: 1 }));
  const p = await ctx.newPage();
  p.on('dialog', (d) => d.accept());
  return p;
}
const shot = (p, n) => p.screenshot({ path: `${OUT}/${n}.png` });
async function postFromBox(p, sub, body) {
  if (await p.isVisible('#b-bd-new')) await p.click('#b-bd-new'); // the catalog's "New thread" opens the box
  await p.click('#bd-body');
  if (sub) await p.fill('#bd-sub', sub);
  await p.fill('#bd-body', body);
  await p.evaluate(() => { document.querySelector('#bd-post-state').textContent = ''; });
  await p.click('#b-bd-post');
  await p.waitForFunction(() => /Posted as No\. \d+|Held|Not posted|offline|busy|Refused/i.test(document.querySelector('#bd-post-state')?.textContent), null, { timeout: 180_000 });
}
try {
  const o = await page(1280, 800);
  await o.goto(`${base}/tor.html`);
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.click('#b-id-save');
  await o.fill('#i-label', 'Owner');
  await o.fill('#i-pass', PASS);
  await o.fill('#i-pass2', PASS);
  await Promise.all([o.waitForEvent('download'), o.click('#b-id-do-save')]);
  await torReady(o, 'owner');
  await shot(o, '01-owner-settings');
  await o.click('#tab-own');
  await o.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  await o.click('#b-board-new');
  await o.waitForSelector('#v-board-new:not([hidden])');
  await shot(o, '02-owner-new-board');
  await o.fill('#bn-title', 'Lab /b/');
  await o.fill('#bn-about', 'A board from the lab: anything goes, be kind.');
  await o.fill('#bn-rules', 'Be kind.\nNo spam.');
  await o.check('#bn-understood');
  await o.click('#b-board-create');
  await o.waitForSelector('#v-board-own:not([hidden])', { timeout: 30_000 });
  await o.evaluate(() => { document.querySelector('#bo-settings').open = true; }); // the owner's settings are folded
  await o.fill('#bo-eff-reply', '20');
  await o.fill('#bo-eff-thread', '40');
  await o.click('#b-bo-efforts');
  const link = await o.inputValue('#bo-link');
  await shot(o, '03-owner-board-empty');

  const r = await page(390, 844, true);
  await r.goto(link.replace('/tor.html#', '/tor.html?r#'));
  await r.waitForFunction(() => /Verified through Tor/.test(document.querySelector('#bd-source')?.textContent), null, { timeout: 180_000 });
  await shot(r, '04-phone-board-empty');
  await postFromBox(r, 'First thread', 'Hello board\n>be me\n>posting from the lab');
  await shot(r, '05-phone-thread-after-post');
  await postFromBox(r, '', 'A reply with a longer text to see how the post wraps on a narrow phone screen, with some more words.');
  await r.click('#b-bd-follow');
  await r.click('#b-bd-catalog').catch(() => {});
  await r.waitForTimeout(1000);
  await shot(r, '06-phone-catalog');
  await r.click('#tab-follow');
  await r.waitForTimeout(1000);
  await shot(r, '07-phone-following');
  await r.locator('#board-follows li').first().click();
  await r.waitForTimeout(2000);
  await r.locator('#bd-catalog li').first().click();
  await r.waitForFunction(() => document.querySelectorAll('#bd-posts li').length >= 2, null, { timeout: 180_000 });
  await r.evaluate(() => document.querySelector('#bd-body').scrollIntoView());
  await shot(r, '08-phone-reply-box');

  await o.click('#tab-own');
  await o.locator('#board-owns li').first().click();
  await o.waitForFunction(() => document.querySelectorAll('#bo-catalog li').length >= 1, null, { timeout: 60_000 });
  await o.locator('#bo-catalog li').first().click();
  await o.waitForTimeout(1500);
  await shot(o, '09-owner-thread');
  await o.evaluate(() => document.querySelector('#bo-modlog')?.scrollIntoView());
  await shot(o, '10-owner-lower');
  const d = await page(1280, 800);
  await d.goto(link.replace('/tor.html#', '/tor.html?d#'));
  await d.waitForFunction(() => /Verified through Tor/.test(document.querySelector('#bd-source')?.textContent), null, { timeout: 180_000 });
  await shot(d, '11-desktop-reader');
  console.log('done');
} catch (e) {
  console.log('failed:', e.message.split('\n')[0]);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
