// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Soak test of Tor mode (docs/P2P-CHAT.md §28): the app is opened and left alone. Two browsers
// open tor.html, connect one chat through Tor, then stay idle for SOAK_MIN minutes (default 30).
// Every minute each side's status is recorded (status chip, peer state, our onion's
// reachability, connection drops, reconnects, Snowflake proxy switches); every SOAK_EVERY
// minutes (default 5) a message goes each way and its delivery time is measured.
// Passes if every message arrives (within 2 min), and at the end both sides are connected and
// reachable.
//
//   node checks/tor-lab/soak_tor.mjs                     offline lab (checks/tor-lab/lab.sh up)
//   LIVE=1 SOAK_MIN=30 node checks/tor-lab/soak_tor.mjs  the real Tor network
import { check, finish, launch, msgWith, openCode, problems, toHome, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

const MINUTES = Number(process.env.SOAK_MIN || 30);
const EVERY = Number(process.env.SOAK_EVERY || 5);
const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];
const counts = { alice: { proxy: 0 }, bob: { proxy: 0 } };

async function open(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext());
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('console', (m) => { if (/proxy lost, switching/.test(m.text())) counts[who].proxy++; });
  await p.goto(`${base}/tor.html`);
  return p;
}

const state = (p) => p.evaluate(() => ({
  status: document.querySelector('#status')?.textContent,
  peer: document.querySelector('#peer-state')?.textContent || '',
  reach: /still publishing/.test(document.querySelector('#tor-state')?.textContent) ? 'publishing' : /Reachable through Tor/.test(document.querySelector('#tor-state')?.textContent) ? 'reachable' : 'no',
  drops: [...document.querySelectorAll('#log li.sys')].filter((l) => /connection dropped/.test(l.textContent)).length,
  back: [...document.querySelectorAll('#log li.sys')].filter((l) => /Reconnected through Tor/.test(l.textContent)).length,
}));

async function probe(from, to, text) {
  const t = Date.now();
  await from.fill('#t-msg', text);
  await from.press('#t-msg', 'Enter');
  return msgWith(to, 'them', text).waitFor({ timeout: 120_000 }).then(() => Date.now() - t, () => -1);
}

try {
  const t0 = Date.now();
  const [a, b] = await Promise.all([open('alice'), open('bob')]);
  await Promise.all([torReady(a, 'alice'), torReady(b, 'bob')]);
  check('both tabs up on Tor, onions hosted', true, `${Date.now() - t0} ms`);
  await toHome(a);
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#t='), null, { timeout: 10_000 });
  await openCode(b);
  await b.fill('#t-code', await a.inputValue('#v-code .link'));
  await b.click('#b-apply');
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: T })));
  check('chat connected through Tor', true);

  console.log(`\n  soak: ${MINUTES} min idle, a message each way every ${EVERY} min`);
  console.log('  min | alice: status / peer / reach / drops / back / proxy | bob: … | A→B ms | B→A ms');
  const lost = [];
  const start = Date.now();
  for (let m = 1; m <= MINUTES; m++) {
    await a.waitForTimeout(Math.max(0, start + m * 60_000 - Date.now()));
    let ab = '', ba = '';
    if (m % EVERY === 0 || m === MINUTES) {
      ab = await probe(a, b, `soak ${m} min A→B`);
      ba = await probe(b, a, `soak ${m} min B→A`);
      if (ab < 0) lost.push(`${m} min A→B`);
      if (ba < 0) lost.push(`${m} min B→A`);
    }
    const [sa, sb] = await Promise.all([state(a), state(b)]);
    const row = (s, w) => `${s.status} / ${s.peer || '-'} / ${s.reach} / ${s.drops} / ${s.back} / ${counts[w].proxy}`;
    console.log(`  ${String(m).padStart(3)} | ${row(sa, 'alice')} | ${row(sb, 'bob')} | ${ab} | ${ba}`);
  }
  const [sa, sb] = await Promise.all([state(a), state(b)]);
  check(`every probe message delivered during ${MINUTES} min`, lost.length === 0, lost.join(', ') || `${Math.floor(MINUTES / EVERY) * 2} messages`);
  check('at the end both sides are connected, and reachable through Tor', sa.status === 'connected' && sb.status === 'connected' && sa.reach !== 'no' && sb.reach !== 'no',
    `drops A ${sa.drops} / B ${sb.drops}, reconnects A ${sa.back} / B ${sb.back}, proxy switches A ${counts.alice.proxy} / B ${counts.bob.proxy}`);
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('soak flow', false, e.message.split('\n')[0]);
  dumpLogs(60);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
