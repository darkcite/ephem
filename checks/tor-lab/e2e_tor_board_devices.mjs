// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Boards on several devices, BD-7 (docs/BOARDS.md G.13), in the offline lab. One saved identity
// on three devices; the vault goes through the lab's stand-in routing host:
//
//   A hosts a board; a poster posts under a trip and A bans it → the vault (v2) lists the board
//   with A's lease → a phone signs in: the board is hosted elsewhere and the phone may not host
//   it → desktop B signs in: "Host this board here" (explicit) reads the whole board from A's
//   onion, numbers continue (above the old lease's bound, G.13.6), the ban survives (the encrypted own block) → A stops (its renewal
//   or its fencing sees B) and says so → a post reaches B with the next number; the banned trip
//   is still refused.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`.
import * as fs from 'node:fs';
import { devices } from 'playwright';
import { check, finish, launch, PASS, problems, toSettings, watch } from '../e2e_lib.mjs';
import { REAL, T, dumpLogs, record, routingStandIn, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

const LEASE_S = 30;
const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const { server: routing, cfg } = await routingStandIn();
const lab = { ...(REAL ? {} : { routing: cfg }), leaseS: LEASE_S, publishDelayMs: 2_000 };
const browsers = [];

async function device(who, extra = {}, context = {}) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true, ...context }), { ...lab, ...extra });
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept());
  await p.goto(`${base}/tor.html`);
  await torReady(p, who);
  return p;
}

async function saveIdentity(p, label) {
  await toSettings(p);
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  const [dl] = await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
  return fs.readFileSync(await dl.path());
}

async function signIn(p, file) {
  await toSettings(p);
  await p.click('#b-id-load');
  await p.setInputFiles('#i-file', { name: 'boards.p2pkey', mimeType: 'application/octet-stream', buffer: file });
  await p.fill('#i-pass-in', PASS);
  await p.click('#b-id-do-load');
  await p.waitForFunction(() => /Board devices/.test(document.querySelector('#id-desc')?.textContent), null, { timeout: 30_000 });
}

const own = (p, fn, ...args) => p.evaluate(([f, a]) => globalThis.ephemBoards.app[f](0, ...a), [fn, args]);
/** A post; once more if Tor itself failed (a circuit, not a refusal by the board). */
const post = (p, name, onion, thread, body, trip = '') => p.evaluate(async ([n, o, t, b, tr]) => {
  let last = '';
  for (let i = 0; i < 2; i++) {
    try {
      const r = await globalThis.ephemBoards.post(n, o, t, t ? '' : 'Thread', b, false, { trip: tr });
      return { no: r.no };
    } catch (e) {
      last = String(e?.message || e);
      if (/E_BOARD_/.test(last)) break;
    }
  }
  return { error: last };
}, [name, onion, thread, body, trip]);

/** The board row in My channels, once the vault (or the store) lists it. */
async function boardRow(p, sub) {
  await p.click('#tab-own');
  await p.waitForFunction((s) => [...document.querySelectorAll('#board-owns li')].some((l) => l.textContent.includes(s)), sub, { timeout: T });
  return p.locator('#board-owns li').first();
}

try {
  // ---- device A hosts a board; a trip is banned ----
  const A = await device('A');
  const keyFile = await saveIdentity(A, 'Board devices');
  await A.click('#tab-own');
  await A.waitForSelector('#v-own-new:not([hidden]) #ch-new:not([hidden])', { timeout: 30_000 });
  await A.click('#b-board-new');
  await A.fill('#bn-title', 'Two devices');
  await A.check('#bn-understood');
  await A.click('#b-board-create');
  await A.waitForFunction(() => /#B=k51/.test(document.querySelector('#bo-link')?.value), null, { timeout: 30_000 });
  await own(A, 'set_efforts', 40, 80);
  const link = await A.inputValue('#bo-link');
  const name = link.match(/#B=(k51[a-z0-9]+)/)[1];
  const onion = link.match(/&o=([a-z2-7]{56}\.onion)/)[1];

  const P = await device('poster');
  await saveIdentity(P, 'Poster');
  const t = await post(P, name, onion, 0, 'first thread');
  const tripped = await post(P, name, onion, t.no, 'a trip post', 'lab');
  await own(A, 'ban', tripped.no, 'test');
  await A.waitForFunction(() => { const v = globalThis.ephemChannel.vault(); return v && JSON.parse(v).boards?.some((b) => b.next_no >= 3); }, null, { timeout: 90_000 });
  const vA = JSON.parse(await A.evaluate(() => globalThis.ephemChannel.vault()));
  check('A hosts the board; the vault (v2) lists it with A\'s lease and its numbers', t.no === 1 && tripped.no === 2 && vA.boards[0].title === 'Two devices' && vA.boards[0].until * 1000 > Date.now(), JSON.stringify(vA.boards[0]).slice(0, 160));

  // ---- a phone: listed, not hostable ----
  const phone = await device('phone', {}, { ...devices['iPhone 13'] });
  await signIn(phone, keyFile);
  const pr = await boardRow(phone, 'hosted on your other device');
  await pr.click();
  await phone.waitForSelector('#v-board-away:not([hidden])', { timeout: 30_000 });
  check('a phone lists the board as hosted elsewhere and may not host it', await phone.isDisabled('#b-ba-host') && await phone.isVisible('#ba-host-note') && !(await phone.evaluate(() => globalThis.ephemBoards.canHost())));

  // ---- desktop B takes over explicitly ----
  const B = await device('B');
  await signIn(B, keyFile);
  const br = await boardRow(B, 'hosted on your other device');
  await br.click();
  await B.waitForSelector('#v-board-away:not([hidden])', { timeout: 30_000 });
  check('B is not given the board silently: it offers "Host this board here"', /hosts this board/.test(await B.textContent('#ba-state')) && !(await B.isDisabled('#b-ba-host')));
  const t0 = Date.now();
  await B.click('#b-ba-host');
  await B.waitForSelector('#v-board-own:not([hidden])', { timeout: T * 2 });
  const st = JSON.parse(await own(B, 'status'));
  const vB = JSON.parse(await own(B, 'owner_view', [t.no]));
  // Numbers continue above the old lease's bound (G.13.6: 120 a minute of the lease left), so
  // posts the old device took before it stopped can never collide with new ones.
  check('B hosts the board with its posts, the ban (encrypted own block), and numbers that cannot collide',
    vB.catalog.length === 1 && vB.threads[0]?.posts.length === 2 && st.next_no >= 3 && st.bans === 1 && st.onion === onion, `${Date.now() - t0} ms; next No. ${st.next_no}, bans ${st.bans}`);

  // ---- A stops ----
  await A.waitForFunction(() => !globalThis.ephemBoards.app.open_boards().length, null, { timeout: 120_000 });
  await A.click('#tab-own');
  const why = await A.evaluate(() => [...document.querySelectorAll('#board-owns li')].map((l) => l.textContent).join('|'));
  check('A stops hosting by itself (lease or fencing) and lists the board as hosted elsewhere', /hosted on your other device/.test(why), why);

  // ---- posting goes on at B ----
  await own(B, 'set_efforts', 40, 80);
  await B.waitForTimeout(2_000);
  const next = await post(P, name, onion, t.no, 'after the move');
  check('a post after the move gets B\'s next number', next.no === st.next_no, JSON.stringify(next));
  const again = await post(P, name, onion, t.no, 'banned trip again', 'lab');
  check('the banned trip is still refused on B', /E_BOARD_REFUSED/.test(again.error || ''), again.error);

  check('no unexpected page errors', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check(`flow: ${e.message.split('\n')[0]}`, false);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
  routing.close();
}
finish();
