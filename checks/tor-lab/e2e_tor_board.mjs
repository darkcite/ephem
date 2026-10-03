// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Boards, BD-3 (docs/BOARDS.md G.16.3), in the offline lab: an owner hosts a board from the main
// app tab (Tor mode, saved identity; R6) on its own onion; two posters in other browsers solve
// the proof of work in Web Workers and submit over Tor; readers verify what is served.
//
//   owner hosts → poster A opens a thread → poster B's thread inside the thread budget is refused
//   (E_BOARD_BUSY) → B replies (bumps), A replies with sage (no bump), the owner posts a
//   thread → bump order as readers verify it → a retried submit gets the original number → the
//   owner deletes an OP (the thread leaves the board) → the store follows every publish (OPFS) →
//   after a reload the board comes back from the store on the same onion, posts intact, no whole-board CAR is served (406).
//   BD-5 (owner moderation): a trip post shows its trip → delete with undo → mass delete from a
//   post on (tombstones, mod log) → a banned trip is refused → pre-moderation holds a post until the
//   owner approves it → trips-only refuses anonymous posts → lock and sticky → the moderation
//   state (ban, switches, efforts) survives a reload in the encrypted own block.
//
// Pruning a full board (150 threads, 30-minute protection) cannot run in lab time: it is the
// native test `crates/board/tests/host.rs::a_full_board_prunes_its_oldest_thread`, the same code.
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

/** A poster's post through the app's boards module (draft, Workers, submit). */
const post = (p, name, onion, thread, sub, body, sage = false, trip = '') => p.evaluate(async ([n, o, t, s, b, g, tr]) => {
  try {
    const r = await globalThis.ephemBoards.post(n, o, t, s, b, g, { trip: tr });
    globalThis.lastDraft = r.draft;
    return { no: r.no, seq: r.seq, held: r.held, trip: r.trip, solveMs: Math.round(r.solveMs), effort: r.effort };
  } catch (e) {
    return { error: String(e?.message || e) };
  }
}, [name, onion, thread, sub, body, sage, trip]);

/** The owner's BoardApp (board 0): `own(o, 'set_locked', no, true)`. */
const own = (o, fn, ...args) => o.evaluate(([f, a]) => globalThis.ephemBoards.app[f](0, ...a), [fn, args]);
const status = async (o) => JSON.parse(await own(o, 'status'));
/** Waits for the deletes' undo window and the publish after it. */
const settle = (p) => p.waitForTimeout(7_000);

/** Saves a chat identity on `p` (posters: trips derive from it). */
async function saveIdentity(p, label) {
  await toSettings(p);
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
}

const read = (p, name, onion, threads) => p.evaluate(async ([n, o, t]) => globalThis.ephemBoards.read(n, o, t), [name, onion, threads]);

try {
  // ---- owner: Tor mode, a saved identity (board keys derive from it, G.4) ----
  const o = await page('owner');
  await o.goto(`${base}/tor.html`);
  await o.waitForSelector('#v-start:not([hidden])');
  await saveIdentity(o, 'Boards');
  await torReady(o, 'owner');
  const t0 = Date.now();
  const { name, onion } = await o.evaluate(async () => {
    const r = await globalThis.ephemBoards.host(0, 'Lab board', 'A board in the offline lab', 'Be kind.');
    // Lab efforts: a few seconds per post on a CI core (the defaults are calibrated for phones).
    globalThis.ephemBoards.app.set_efforts(0, 40, 80);
    return r;
  });
  await o.waitForFunction(() => /reachable|degraded/.test(globalThis.ephemBoards.app.reach(0)), null, { timeout: T });
  check('the owner hosts a board on its own onion (main app tab)', /^k51/.test(name) && /^[a-z2-7]{56}\.onion$/.test(onion), `${Date.now() - t0} ms to reachable`);

  // ---- posters A and B, each in its own browser ----
  const [a, b] = [await page('poster A'), await page('poster B')];
  for (const [p, who] of [[a, 'poster A'], [b, 'poster B']]) {
    await p.goto(`${base}/tor.html`);
    await p.waitForSelector('#v-start:not([hidden])');
    if (p === a) await saveIdentity(a, 'Poster A'); // A posts under a trip later
    await torReady(p, who);
  }
  const ta = await post(a, name, onion, 0, 'First thread', 'Hello from poster A');
  check('poster A opens a thread (PoW solved in Workers, submitted over Tor)', ta.no === 1 && ta.seq > 0, JSON.stringify(ta));
  const early = await post(b, name, onion, 0, 'Too soon', 'a second thread at once');
  check('a second thread inside the thread budget is refused', /E_BOARD_BUSY/.test(early.error || ''), early.error);
  const rb = await post(b, name, onion, ta.no, '', 'Reply from B, bumps');
  check('poster B replies', rb.no === 2, JSON.stringify(rb));
  await o.waitForTimeout(1_500); // bumps are in seconds: the owner's thread a second later
  const owner = await o.evaluate(() => globalThis.ephemBoards.app.post(0, 0, 'Owner thread', 'From the owner', false));
  await o.waitForTimeout(2_000); // the next publish
  const ra = await post(a, name, onion, ta.no, '', 'Reply from A, sage', true);
  check('poster A replies with sage', ra.no === owner + 1, JSON.stringify(ra));

  // A retried submit (the answer was lost): the original number, not a refusal.
  const again = await a.evaluate(async () => globalThis.ephemBoards.resend(globalThis.lastDraft));
  check('a retried submit is answered with the original {no, seq}', again.no === ra.no && again.seq === ra.seq, JSON.stringify(again));

  // ---- a reader (poster B's tab) verifies it all ----
  const v = await read(b, name, onion, [ta.no, owner]);
  const th = v.threads.find((t) => t.no === ta.no);
  check('readers verify the board: titles, posts, sage, capcode',
    v.title === 'Lab board' && th.posts.map((p) => p.body).join('|') === 'Hello from poster A|Reply from B, bumps|Reply from A, sage'
      && th.posts[2].sage === true && v.threads.find((t) => t.no === owner).posts[0].cap === 1);
  check('bump order: the owner\'s thread is newer than A\'s last non-sage bump', v.catalog.map((c) => c.no).join(',') === `${owner},${ta.no}`, v.catalog.map((c) => `${c.no}:${c.bump}`).join(' '));

  // ---- the owner deletes an OP: its thread leaves the board ----
  await own(o, 'delete', owner);
  await settle(o);
  const v2 = await read(a, name, onion, []);
  check('an OP deleted by the owner takes its thread off the board', v2.catalog.map((c) => c.no).join(',') === `${ta.no}` && v2.sequence > v.sequence);

  // ---- no whole-board CAR (B-M10), seen as Tor Browser would (the lab's C Tor client) ----
  if (!REAL) {
    const car = await torBrowserGet(onion, `/ipfs/${v2.root}?format=car`);
    const idx = await torBrowserGet(onion, '/');
    check('the root as one CAR is refused (406); the onion\'s page says it is a board', car.status === 406 && idx.status === 200 && /Ephem board/.test(idx.body), `${car.status}`);
  }

  // ---- BD-5: owner moderation ----
  // A trip: a stable key from A's identity, shown as ! + 16 characters.
  const tr = await post(a, name, onion, ta.no, '', 'A speaks under a trip', false, 'lab');
  const vt = await read(b, name, onion, [ta.no]);
  const shown = vt.threads[0].posts.find((x) => x.no === tr.no);
  check('a trip post: readers see the trip (! + 16 characters)', /^![a-z2-7]{16}$/.test(shown?.trip || '') && shown.trip === tr.trip, shown?.trip);
  // Delete with undo: undone in the window, the post stays.
  await own(o, 'delete', rb.no);
  const undone = await own(o, 'undo', rb.no);
  // Mass delete from a number on: both of these spam posts become tombstones.
  const s1 = await post(b, name, onion, ta.no, '', 'spam one');
  const s2 = await post(b, name, onion, ta.no, '', 'spam two');
  const queued = await own(o, 'delete_from', s1.no);
  await settle(o);
  const vm = await read(b, name, onion, [ta.no]);
  const posts = vm.threads[0].posts;
  check('the owner mass-deletes (from a post on): readers see tombstones; an undone delete keeps its post',
    undone === 1 && queued === 2 && [s1.no, s2.no].every((n) => posts.find((x) => x.no === n)?.del === 1) && posts.some((x) => x.no === rb.no && x.del === 0)
      && vm.modlog.filter((m) => m.act === 'delete').length >= 2, `queued ${queued}`);
  // Ban the trip: its next post is refused.
  await own(o, 'ban', tr.no, 'lab test');
  await o.waitForTimeout(1_500);
  const banned = await post(a, name, onion, ta.no, '', 'banned trip', false, 'lab');
  check('a banned trip is refused (E_BOARD_REFUSED)', /E_BOARD_REFUSED/.test(banned.error || ''), banned.error);
  // Pre-moderation: held, then approved by the owner.
  await own(o, 'set_switches', false, false, false, false, true, false);
  await o.waitForTimeout(1_500);
  const hp = await post(b, name, onion, ta.no, '', 'please approve me');
  const heldList = JSON.parse(await own(o, 'held'));
  const approvedNo = await own(o, 'approve', 0);
  await o.waitForTimeout(2_000);
  const vh = await read(a, name, onion, [ta.no]);
  check('pre-moderation: the post is held (no number), then the owner approves it and readers see it',
    hp.held === true && heldList.length === 1 && heldList[0].body === 'please approve me' && vh.threads[0].posts.some((x) => x.no === approvedNo && x.body === 'please approve me'));
  // Trips-only: an anonymous post is refused (E_BOARD_PAUSED); lock, sticky and the mod log.
  await own(o, 'set_switches', false, false, true, false, false, false);
  await own(o, 'set_sticky', ta.no, true);
  await o.waitForTimeout(1_500);
  const anonRefused = await post(b, name, onion, ta.no, '', 'anon under trips-only');
  check('trips-only: an anonymous post is refused (E_BOARD_PAUSED)', /E_BOARD_PAUSED/.test(anonRefused.error || ''), anonRefused.error);
  await own(o, 'set_switches', false, false, false, false, false, false);
  await own(o, 'set_locked', ta.no, true);
  await o.waitForTimeout(1_500);
  const locked = await post(b, name, onion, ta.no, '', 'into a locked thread');
  const vl = await read(b, name, onion, []);
  check('a locked thread refuses replies; sticky and lock show in the catalog and the mod log',
    /E_BOARD_REFUSED/.test(locked.error || '') && vl.catalog[0].st && vl.catalog[0].lk && ['ban', 'sticky', 'lock', 'approve'].every((a) => vl.modlog.some((m) => m.act === a)), locked.error);
  await own(o, 'set_locked', ta.no, false);
  // State to survive the reload (the encrypted own block): the ban, trips-only, the efforts.
  await own(o, 'set_switches', false, false, true, false, false, true);
  await o.waitForTimeout(1_500);

  // ---- the store follows every publish; a reload brings the board back ----
  const kept = await o.evaluate(async (n) => ({ ...(await globalThis.ephemBoards.stored(n)), served: JSON.parse(globalThis.ephemBoards.app.status(0)).blocks }), name);
  check('the store holds exactly the served blocks (OPFS, one file per block; unreachable ones deleted)', kept.record > 0 && kept.blocks === kept.served, JSON.stringify(kept));
  await o.reload();
  await o.waitForSelector('#v-start:not([hidden])');
  await toSettings(o);
  await o.locator('#slots li', { hasText: 'Boards' }).locator('button', { hasText: 'Sign in' }).click();
  await o.locator('#slots li input[type=password]').fill(PASS);
  await o.locator('#slots li', { hasText: 'Boards' }).locator('button', { hasText: 'Sign in' }).click();
  await o.waitForFunction(() => /Boards/.test(document.querySelector('#id-desc')?.textContent));
  await torReady(o, 'owner (reload)');
  const back = await o.evaluate(() => globalThis.ephemBoards.reopen(0));
  await o.waitForFunction(() => /reachable|degraded/.test(globalThis.ephemBoards.app.reach(0)), null, { timeout: T });
  const st = await status(o);
  const sw = JSON.parse(await own(o, 'switches'));
  check('after a reload the moderation state is back (encrypted own block): ban, trips-only, panic choice, efforts',
    st.bans === 1 && sw.trips_only && sw.panic_trips && st.base_reply === 40 && st.base_thread === 80, JSON.stringify({ bans: st.bans, sw, base: st.base_reply }));
  await own(o, 'set_switches', false, false, false, false, false, false);
  const v3 = await read(b, name, onion, [ta.no]);
  const before = vl.next_no;
  check('after a reload the board comes back from the store, same onion, posts intact',
    back.onion === onion && v3.threads[0]?.posts.length === posts.length + 1 && v3.sequence > vl.sequence);
  await o.waitForTimeout(1_500);
  const after = await post(b, name, onion, ta.no, '', 'After the reload');
  check('posting continues after the reload (numbers go on)', after.no === before, JSON.stringify(after));

  console.log(`  solve times: ${[ta, rb, ra, tr, after].map((x) => `${x.solveMs} ms @${x.effort}`).join(', ')}`);
  check('no unexpected page errors', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check(`flow: ${e.message.split('\n')[0]}`, false);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
