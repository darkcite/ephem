// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Boards, BD-6 (docs/BOARDS.md G.11, G.12), through the app's screens in the offline lab:
//
//   owner (Tor mode, saved identity) creates a board in My channels (warnings acknowledged) →
//   a reader opens the #B= link: the catalog, a new thread from the reply box (the proof of work
//   solved while typing), a reply, greentext → Follow (the board is in Following, ▦) → the owner
//   sees the posts and moderates (delete with its undo bar) → a second reader mirrors the board
//   and the owner signs the mirror in → Tor Browser reads the plain pages (catalog and thread,
//   no scripts) → the owner's tab closes → a new reader opens the link and reads from the
//   mirror; posting says E_BOARD_OFFLINE and keeps the draft.
//
// Stale mode (a record past its 72 h) is the native test `stale_mode_after_expiry`.
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 for the real Tor network.
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { REAL, T, dumpLogs, record, serveTor, torBrowserGet, torContext, torReady, unexpected } from './tor_env.mjs';

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

const bodies = (p) => p.$$eval('#bd-posts li .body', (ls) => ls.map((l) => l.textContent));

/** Types into the reply box (which starts the proof of work), then posts. */
async function postFromBox(p, sub, body) {
  await p.click('#bd-body');
  if (sub) await p.fill('#bd-sub', sub);
  await p.fill('#bd-body', body);
  await p.evaluate(() => { document.querySelector('#bd-post-state').textContent = ''; });
  await p.click('#b-bd-post');
  await p.waitForFunction(() => /Posted as No\. \d+|Held|Not posted|offline|busy|Refused|paused|more work/i.test(document.querySelector('#bd-post-state')?.textContent), null, { timeout: T });
  return p.textContent('#bd-post-state');
}

try {
  // ---- owner: a board from My channels ----
  const o = await page('owner');
  await o.goto(`${base}/tor.html`);
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.click('#b-id-save');
  await o.fill('#i-label', 'Board owner');
  await o.fill('#i-pass', PASS);
  await o.fill('#i-pass2', PASS);
  await Promise.all([o.waitForEvent('download'), o.click('#b-id-do-save')]);
  await torReady(o, 'owner');
  await o.click('#tab-own');
  await o.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 }); // My channels has opened
  await o.click('#b-board-new');
  await o.waitForSelector('#v-board-new:not([hidden])');
  await o.fill('#bn-title', 'Lab /b/');
  await o.fill('#bn-about', 'A board from the lab');
  await o.fill('#bn-rules', 'Be kind.');
  await o.click('#b-board-create');
  check('the warnings must be acknowledged', /I understand/.test(await o.textContent('#error')));
  await o.check('#bn-understood');
  await o.click('#b-board-create');
  await o.waitForSelector('#v-board-own:not([hidden])', { timeout: 30_000 });
  // The link is there at once; the reader below proves the onion answers (arti's own status can
  // lag behind its published descriptor in the lab).
  await o.waitForFunction(() => /#B=k51/.test(document.querySelector('#bo-link')?.value), null, { timeout: 30_000 });
  // Lab efforts: the defaults are calibrated for phones.
  await o.fill('#bo-eff-reply', '40');
  await o.fill('#bo-eff-thread', '80');
  await o.click('#b-bo-efforts');
  const link = await o.inputValue('#bo-link');
  const onion = link.match(/&o=([a-z2-7]{56}\.onion)/)?.[1];
  check('the owner creates a board in My channels: a #B= link, listed with ▦',
    /\/tor\.html#B=k51[a-z0-9]+&o=[a-z2-7]{56}\.onion$/.test(link) && /▦ Lab \/b\//.test(await o.textContent('#board-owns')), link.slice(-90));

  // ---- reader A: the link, the catalog, a thread from the reply box ----
  const a = await page('reader A');
  await a.goto(link.replace('/tor.html#', '/tor.html?a#'));
  await a.waitForFunction(() => /Verified through Tor/.test(document.querySelector('#bd-source')?.textContent), null, { timeout: T });
  check('a reader opens the link: the board, verified', (await a.textContent('#bd-title')).startsWith('▦ Lab /b/ · …') && (await a.textContent('#bd-rules')) === 'Be kind.');
  const s1 = await postFromBox(a, 'First thread', 'Hello board\n>be me\n>posting from the lab');
  await a.waitForFunction(() => document.querySelectorAll('#bd-posts li').length >= 1, null, { timeout: T });
  const gt = await a.$$eval('#bd-posts .gt', (g) => g.map((x) => x.textContent));
  check('a new thread from the reply box (work done while typing); greentext shown', /No\. 1/.test(s1) && gt.join('|') === '>be me|>posting from the lab', s1);
  const s2 = await postFromBox(a, '', 'A reply');
  await a.waitForFunction(() => [...document.querySelectorAll('#bd-posts .body')].some((b) => b.textContent === 'A reply'), null, { timeout: T });
  check('a reply in the thread', /No\. 2/.test(s2), s2);
  await a.click('#b-bd-follow');
  await a.click('#tab-follow');
  check('Follow: the board is in Following, marked ▦', /▦ Lab \/b\//.test(await a.textContent('#board-follows')));

  // ---- the owner sees it and moderates (delete with undo) ----
  await o.click('#tab-own');
  await o.locator('#board-owns li', { hasText: 'Lab /b/' }).click();
  await o.waitForFunction(() => document.querySelectorAll('#bo-catalog li').length === 1, null, { timeout: 30_000 });
  await o.locator('#bo-catalog li').first().click();
  await o.waitForFunction(() => document.querySelectorAll('#bo-thread li').length === 2);
  await o.locator('#bo-thread li', { hasText: 'A reply' }).locator('button', { hasText: 'Delete' }).click();
  const undoShown = await o.isVisible('#bo-undo');
  await o.waitForFunction(() => /deleted/.test(document.querySelector('#bo-thread')?.textContent) && /delete No\. 2/.test(document.querySelector('#bo-modlog')?.textContent), null, { timeout: 30_000 });
  check('the owner deletes a reply (an undo bar first), the mod log says so', undoShown);
  await a.click('#b-bd-refresh');
  await a.waitForFunction(() => [...document.querySelectorAll('#bd-posts li')].some((l) => l.classList.contains('deleted')), null, { timeout: T });
  check('readers see it deleted', (await bodies(a)).includes('(deleted)'));

  // ---- reader B mirrors; the owner signs the mirror in ----
  const b = await page('reader B');
  await b.goto(link.replace('/tor.html#', '/tor.html?b#'));
  await b.waitForFunction(() => /Verified through Tor/.test(document.querySelector('#bd-source')?.textContent), null, { timeout: T });
  await b.click('#b-bd-mirror');
  await b.waitForFunction(() => /Mirroring on [a-z2-7]{56}\.onion/.test(document.querySelector('#bd-mirror-note')?.textContent), null, { timeout: T });
  const mirror = (await b.textContent('#bd-mirror-note')).match(/([a-z2-7]{56}\.onion)/)[1];
  await o.fill('#bo-mirrors', mirror);
  await o.click('#b-bo-mirrors');
  await o.waitForFunction((m) => document.querySelector('#bo-link')?.value.includes(`&m=${m}`), mirror, { timeout: 30_000 });
  const link2 = await o.inputValue('#bo-link');
  check('a reader mirrors the board; the owner signs the mirror into the board (the link names it)', link2.includes(`&m=${mirror}`));

  // ---- Tor Browser: the plain pages ----
  if (!REAL) {
    const cat = await torBrowserGet(onion, '/');
    const th = await torBrowserGet(onion, '/t/1');
    const mp = await torBrowserGet(mirror, '/t/1');
    check('Tor Browser: the catalog and a thread as plain pages (no scripts), also from the mirror',
      cat.status === 200 && /Content-Security-Policy: default-src 'none'/.test(cat.headers) && cat.body.includes('First thread') && !/<script/i.test(cat.body)
        && th.status === 200 && th.body.includes('&gt;be me') && th.body.includes('(deleted)') && mp.status === 200 && mp.body.includes('Served by a mirror'), `${cat.status} ${th.status} ${mp.status}`);
  }

  // ---- the owner goes offline: read from the mirror, posting keeps the draft ----
  await b.waitForTimeout(12_000); // the mirror's next pull (10 s) has the signed mirror list
  await o.close();
  const c = await page('reader C');
  const t0 = Date.now();
  await c.goto(link2.replace('/tor.html#', '/tor.html?c#'));
  await c.waitForFunction(() => /Verified through Tor/.test(document.querySelector('#bd-source')?.textContent), null, { timeout: T });
  check('owner offline: a new reader reads the board from the mirror', (await c.textContent('#bd-title')).startsWith('▦ Lab /b/ · …') && (await c.textContent('#bd-catalog')).includes('First thread'), `${Date.now() - t0} ms`);
  await c.locator('#bd-catalog li').first().click();
  await c.waitForFunction(() => document.querySelectorAll('#bd-posts li').length === 2, null, { timeout: T });
  const said = await postFromBox(c, '', 'written while the host is away');
  check('posting while the host is offline says so (E_BOARD_OFFLINE) and keeps the draft',
    /host is offline/.test(said) && (await c.inputValue('#bd-body')) === 'written while the host is away', said);

  check('no unexpected page errors', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check(`flow: ${e.message.split('\n')[0]}`, false);
  for (const b of browsers) {
    for (const ctx of b.contexts()) {
      for (const p of ctx.pages()) {
        const s = await p.evaluate(() => ({
          url: location.hash.slice(0, 20),
          views: [...document.querySelectorAll('#pane > .view')].filter((v) => !v.hidden).map((v) => v.id),
          source: document.querySelector('#bd-source')?.textContent,
          state: document.querySelector('#bo-state')?.textContent,
          post: document.querySelector('#bd-post-state')?.textContent,
          error: document.querySelector('#error')?.textContent,
          tor: document.querySelector('#tor-state')?.textContent,
        })).catch((x) => String(x));
        console.log('  state:', JSON.stringify(s));
      }
    }
  }
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
