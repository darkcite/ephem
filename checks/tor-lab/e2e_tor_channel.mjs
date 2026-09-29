// Public channels (docs/P2P-CHAT.md §27, Appendix D; CH-2…CH-5) in the offline lab:
//
//   owner (saved identity, remembered in this browser) opens channel.html, signs in, creates a
//   channel (warnings acknowledged), posts 3, deletes 1 → the channel is served on its own onion
//   from the tab → a reader in another browser opens the link and verifies it over Tor → the
//   reader mirrors it (a second onion, in the reader's tab) → the owner signs the mirror into
//   the manifest → the owner closes the tab → a new reader reads the channel from the mirror.
//   The owner's store survives a reload (OPFS) and the backup (CAR + record) exports.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 for the real Tor network.
import { PASS, check, finish, launch, problems, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, serveTor, torContext, unexpected } from './tor_env.mjs';

const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];

async function page(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }));
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept());
  return p;
}

const posts = (p) => p.$$eval('#r-posts li .body, #o-posts li .body', (ls) => ls.map((l) => l.textContent));

try {
  // ---- owner: a saved identity remembered in this browser (the chat app's key file) ----
  const o = await page('owner');
  await o.goto(`${base}/`);
  await o.waitForSelector('#v-start:not([hidden])');
  await o.click('#b-id-save');
  await o.fill('#i-label', 'Channels');
  await o.fill('#i-pass', PASS);
  await o.fill('#i-pass2', PASS);
  await Promise.all([o.waitForEvent('download'), o.click('#b-id-do-save')]);
  await o.goto(`${base}/channel.html`);
  await o.waitForSelector('#v-own:not([hidden])');
  await o.click('#slots li button');
  await o.fill('#i-pass', PASS);
  await o.click('#b-signin');
  await o.waitForSelector('#ch-new:not([hidden])', { timeout: 30_000 });
  check('owner signs in on channel.html with a remembered identity', true);
  await o.fill('#i-title', 'Lab news');
  await o.fill('#i-about', 'Posts from the offline Tor lab');
  await o.click('#b-create');
  check('the warnings must be acknowledged', /understand/.test(await o.textContent('#error')));
  await o.check('#c-understood');
  await o.click('#b-create');
  await o.waitForSelector('#ch-open:not([hidden])');
  for (const t of ['first post', 'second post', 'third post']) {
    await o.fill('#t-post', t);
    await o.click('#b-post');
    await o.waitForFunction((t) => [...document.querySelectorAll('#o-posts .body')].some((b) => b.textContent === t), t);
  }
  await o.locator('#o-posts li', { hasText: 'second post' }).locator('button', { hasText: 'Delete' }).click();
  await o.waitForFunction(() => /deleted by the owner/.test(document.querySelector('#o-posts').textContent));
  const t0 = Date.now();
  await o.waitForFunction(() => /Online through Tor/.test(document.querySelector('#o-serving').textContent), null, { timeout: T });
  const link = await o.inputValue('#o-link');
  check('channel created, 3 posts, 1 deleted, online on its own onion', /#c=k51.*&o=[a-z2-7]{56}\.onion$/.test(link), `${Date.now() - t0} ms to online`);

  // ---- reader over Tor ----
  const r = await page('reader');
  const t1 = Date.now();
  await r.goto(link);
  await r.waitForFunction(() => /Verified/.test(document.querySelector('#r-source').textContent), null, { timeout: T });
  const got = await posts(r);
  check('a reader verifies the channel over Tor (newest first, deleted shown as such)',
    (await r.textContent('#r-title')) === 'Lab news' && got[0] === 'third post' && /deleted by the owner/.test(got[1]) && got[2] === 'first post', `${Date.now() - t1} ms`);

  // ---- mirror ----
  await r.click('#b-mirror');
  await r.waitForSelector('#mirror-note:not([hidden])', { timeout: 60_000 });
  const mirror = (await r.textContent('#mirror-text')).match(/[a-z2-7]{56}\.onion/)[0];
  check('the reader mirrors it on a second onion of its tab', !!mirror, mirror);
  await o.fill('#i-mirrors', mirror);
  await o.click('#b-mirrors');
  await o.waitForFunction((m) => document.querySelector('#o-link').value.includes(m), mirror);
  const withMirror = await o.inputValue('#o-link');
  await r.click('#b-refresh');
  await r.waitForFunction(() => /version 6/.test(document.querySelector('#r-source').textContent), null, { timeout: T });
  check('owner signs the mirror list; the reader sees the new version (and mirrors it)', true, await r.textContent('#r-source'));

  // ---- C-P3: a channel of 1 000 posts over an onion ----
  const t3 = Date.now();
  await o.evaluate(() => { for (let i = 1; i <= 1000; i++) globalThis.ephemChannel.post(`bulk post ${i}`, 0); });
  const ownerMs = Date.now() - t3;
  const size = await o.evaluate(() => globalThis.ephemChannel.car().length);
  const t4 = Date.now();
  await r.click('#b-refresh');
  await r.waitForFunction(() => document.querySelectorAll('#r-posts li').length >= 1003, null, { timeout: T });
  check('C-P3: 1 000 more posts; a reader fetches and verifies them over the onion', true,
    `owner ${ownerMs} ms for 1 000 posts, CAR ${Math.round(size / 1024)} KB; reader ${Date.now() - t4} ms (fetch + verify + render)`);

  // ---- the owner's store survives a reload; the backup exports ----
  await o.reload();
  await o.waitForSelector('#v-own:not([hidden])');
  await o.click('#slots li button');
  await o.fill('#i-pass', PASS);
  await o.click('#b-signin');
  await o.waitForSelector('#ch-open:not([hidden])', { timeout: 30_000 });
  check('after a reload the owner\'s channel is back from its store', (await posts(o)).length === 3);
  const [d1] = await Promise.all([o.waitForEvent('download'), o.click('#b-export')]);
  check('backup exports as channel.car (+ record)', d1.suggestedFilename() === 'channel.car');

  // ---- owner offline: a new reader gets it from the mirror ----
  await o.close();
  const n = await page('reader2');
  const t2 = Date.now();
  await n.goto(withMirror);
  await n.waitForFunction(() => /Verified/.test(document.querySelector('#r-source').textContent), null, { timeout: T });
  check('owner offline: a new reader reads the channel from the mirror (addresses tried in parallel)', (await posts(n)).includes('third post'), `${Date.now() - t2} ms`);
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('channel flow', false, e.message.split('\n')[0]);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
