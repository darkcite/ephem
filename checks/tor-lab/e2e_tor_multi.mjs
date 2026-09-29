// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// APP-E2E-MULTI over Tor (docs/P2P-CHAT.md Appendix F.3.2) in the offline lab: Alice's one
// onion service carries several chats. Bob and Carol each open one of her Tor invites; each
// incoming stream finds its chat; a lost stream in Bob's chat is redialled without touching
// Carol's; later Carol, a contact now, dials Alice while Alice is busy with Bob: a new chat
// appears in Alice's list (not on screen, since she is in another chat) and opens on a tap.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 for the real Tor network.
import { PASS, check, finish, launch, msgWith, problems, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];

async function open(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }));
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.on('dialog', (d) => d.accept(who === 'carol' ? 'Alice' : 'Carol'));
  await p.goto(`${base}/tor.html`);
  return p;
}

async function saveIdentity(p, label) {
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
}

async function invite(a) {
  await a.click('#b-new');
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#t='), null, { timeout: 10_000 });
  return a.inputValue('#v-code .link');
}

const say = async (p, t) => {
  await p.fill('#t-msg', t);
  await p.press('#t-msg', 'Enter');
};
const pick = (p, text) => p.locator('#chats li', { hasText: text }).click();

try {
  const [a, b, c] = await Promise.all([open('alice'), open('bob'), open('carol')]);
  await Promise.all([torReady(a, 'alice'), torReady(b, 'bob'), torReady(c, 'carol')]);
  await saveIdentity(a, 'A');
  await saveIdentity(c, 'C');
  for (const [p, n] of [[a, 'Alice'], [b, 'Bob'], [c, 'Carol']]) {
    await p.fill('#i-nick', n);
    await p.dispatchEvent('#i-nick', 'change');
  }

  // Two invites of one onion, open at the same time; each dial finds its own chat.
  const forBob = await invite(a);
  const forCarol = await invite(a);
  await b.fill('#t-code', forBob);
  await b.click('#b-apply');
  await c.fill('#t-code', forCarol);
  await c.click('#b-apply');
  await Promise.all([b, c].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: T })));
  await a.waitForFunction(() => document.querySelectorAll('#chats li.ok').length === 2, null, { timeout: T });
  check('two Tor invites of one onion: Bob and Carol each reach their own chat', true);
  await say(b, 'Bob here');
  await say(c, 'Carol here');
  await pick(a, 'Bob');
  await msgWith(a, 'them', 'Bob here').waitFor({ timeout: 60_000 });
  check('Bob\'s chat holds only Bob\'s messages', !(await a.textContent('#log')).includes('Carol here'));
  await pick(a, 'Carol');
  await msgWith(a, 'them', 'Carol here').waitFor({ timeout: 60_000 });

  // A lost stream in Bob's chat: redialled, queued message delivered; Carol's chat unaffected.
  await pick(a, 'Bob');
  await b.click('#b-info');
  await b.click('#b-drop');
  await say(b, 'after the drop');
  await msgWith(a, 'them', 'after the drop').waitFor({ timeout: T });
  await pick(a, 'Carol');
  await say(a, 'still with you, Carol');
  await msgWith(c, 'them', 'still with you, Carol').waitFor({ timeout: 60_000 });
  check('a redial in Bob\'s chat leaves Carol\'s chat untouched', true);

  // Each saves the other as a contact; Carol leaves, then dials Alice while Alice is busy with
  // Bob (a contact dial is taken only from a contact, §28.7).
  await a.click('#b-save-contact');
  await a.waitForFunction(() => document.querySelector('#b-save-contact')?.hidden);
  await c.click('#b-save-contact');
  await c.waitForFunction(() => document.querySelector('#b-save-contact')?.hidden);
  await c.click('#b-leave');
  await c.click('#b-again');
  await pick(a, 'Bob');
  const before = await a.$$eval('#chats li', (l) => l.length);
  await c.locator('#contacts li', { hasText: 'Alice' }).locator('button', { hasText: 'Connect' }).click();
  await c.waitForSelector('#v-chat:not([hidden])', { timeout: T });
  await a.waitForFunction((n) => document.querySelectorAll('#chats li').length > n - 1 && [...document.querySelectorAll('#chats li')].some((l) => /Carol/.test(l.textContent) && /connected/.test(l.textContent)), before, { timeout: T });
  check('a contact dials while Alice is in another chat: a new chat in her list, Bob\'s stays on screen',
    /Bob/.test(await a.textContent('#peer')));
  await say(c, 'called you');
  await a.locator('#chats li', { hasText: 'Carol' }).filter({ has: a.locator('.badge') }).first().click({ timeout: 60_000 });
  await msgWith(a, 'them', 'called you').waitFor({ timeout: 60_000 });
  check('a tap opens it; the message is there', true);

  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('Tor multi-chat flow', false, e.message.split('\n')[0]);
  dumpLogs(60);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
