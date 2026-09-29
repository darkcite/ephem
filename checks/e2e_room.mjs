// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// End-to-end test of rooms (MVP-3, docs/P2P-CHAT.md §14) in real Chromium, four tabs:
//
//   owner A creates a room · members B and C and observer D join by invite + answer · members
//   are introduced through the owner (sealed signalling) and connect directly (full mesh) ·
//   room messages with sender labels, replies, delivery k/N · observer is read-only · owner
//   moderation (delete for everyone) and room timer · removal · a member leaving · the owner
//   closing the room.
//
// Usage: node checks/e2e_room.mjs   (build first with ./build.sh; E2E_BROWSER=chrome for installed Chrome)
import { check, finish, launch, msgWith, openSettings, problems, serve, toSettings, watch } from './e2e_lib.mjs';

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
const accept = (page) => page.once('dialog', (d) => d.accept());
const rows = (page) => page.$$eval('#members li', (ls) => ls.map((l) => l.textContent));
const allDirect = (page, n) => page.waitForFunction((n) => {
  const ls = [...document.querySelectorAll('#members li')];
  return ls.length === n && ls.filter((l) => /\(you\)/.test(l.textContent) || /direct/.test(l.textContent)).length === n;
}, n, { timeout: 30000 });

/** Owner `a` invites `p` (observer or member); `p` answers; resolves when `p` is in the room. */
async function join(a, p, observer) {
  await a.click(observer ? '#b-room-observer' : '#b-room-invite');
  await a.waitForFunction(() => document.querySelector('#room-invite .link')?.value.includes('#i='), null, { timeout: 15000 });
  await openSettings(p);
  await p.fill('#t-code', await a.inputValue('#room-invite .link'));
  accept(p); // "every member will see your IP" (§29.2)
  await p.click('#b-apply');
  await p.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#a='), null, { timeout: 15000 });
  await a.fill('#t-room-answer', await p.inputValue('#v-code .link'));
  await a.click('#b-room-answer');
  await p.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 });
  await p.waitForFunction(() => document.querySelectorAll('#members li').length >= 2, null, { timeout: 10000 });
}

const pages = {};
try {
  for (const [who, nick] of [['a', 'Alice'], ['b', 'Bob'], ['c', 'Carol'], ['d', 'Dave']]) {
    const p = await (await browser.newContext()).newPage();
    watch(p, who);
    await p.goto(`${base}/app/`);
    await p.waitForSelector('#v-start:not([hidden])');
    await toSettings(p);
    await p.fill('#i-nick', nick);
    await p.dispatchEvent('#i-nick', 'change');
    pages[who] = p;
  }
  const { a, b, c, d } = pages;

  // ---- owner creates the room, B joins ----
  await openSettings(a);
  await a.click('#b-room');
  await a.waitForSelector('#room:not([hidden])');
  check('owner opens a room', /1 \/ 16/.test(await a.textContent('#room-count')) && (await a.textContent('#room-role')) === 'owner');
  await join(a, b, false);
  await allDirect(a, 2);
  const sasA = (await rows(a)).find((r) => /Bob/.test(r)).match(/SAS (\d{3} \d{3})/)?.[1];
  check('member joins; the owner sees its safety code', sasA === (await b.textContent('#sas-digits')), `${sasA} vs ${await b.textContent('#sas-digits')}`);
  check('member sees the room and its role', (await b.textContent('#room-role')) === 'member' && /Room of/.test(await b.textContent('#peer')));

  // ---- C joins: introduced to B through the owner, after agreeing to connect ----
  await join(a, c, false);
  await c.waitForSelector('#room-confirm:not([hidden])', { timeout: 10000 });
  check('newcomer is asked before connecting to the other members', /1 other member/.test(await c.textContent('#room-confirm-text')));
  await c.click('#b-room-connect');
  await Promise.all([allDirect(a, 3), allDirect(b, 3), allDirect(c, 3)]);
  check('B and C connect directly (introduced by the owner)', true);

  // ---- D joins as observer ----
  await join(a, d, true);
  await d.waitForSelector('#room-confirm:not([hidden])', { timeout: 10000 });
  await d.click('#b-room-connect');
  await Promise.all([allDirect(a, 4), allDirect(b, 4), allDirect(c, 4), allDirect(d, 4)]);
  check('full mesh of 4', true, (await rows(d)).join(' | '));
  // T2 (§13): B loses its direct links to C and D; they come back through the owner's signalling.
  await b.click('#b-info');
  await b.click('#b-drop');
  await b.waitForFunction(() => /reconnecting|connecting/.test(document.querySelector('#members')?.textContent), null, { timeout: 5000 });
  await Promise.all([allDirect(b, 4), allDirect(c, 4), allDirect(d, 4)]);
  await b.click('#b-info');
  check('lost member links resume automatically (T2 via the owner)', true);
  check('observer is read-only', (await d.textContent('#room-role')) === 'observer' && (await d.isHidden('#f-send')));

  // ---- messages ----
  await b.fill('#t-msg', 'hello room');
  await b.press('#t-msg', 'Enter');
  for (const p of [a, c, d]) await msgWith(p, 'them', 'hello room').waitFor({ timeout: 8000 });
  check('a room message reaches everyone, labelled with its sender', /Bob/.test(await msgWith(c, 'them', 'hello room').locator('.who').textContent()));
  await b.waitForFunction(() => document.querySelector('#log li.me .tick')?.textContent === '✓ 3/3', null, { timeout: 8000 });
  check('delivery shown as k/N', true);

  await msgWith(c, 'them', 'hello room').click();
  await c.click('#log .acts button:text("Reply")');
  await c.fill('#t-msg', 'hi Bob');
  await c.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'hi Bob').waitFor({ timeout: 8000 });
  check('replies quote a third member’s message', /Bob.*: hello room/.test(await msgWith(a, 'them', 'hi Bob').locator('.quote').textContent()));
  await msgWith(d, 'them', 'hi Bob').waitFor({ timeout: 8000 });

  // ---- owner moderation and timer ----
  await msgWith(a, 'them', 'hi Bob').click();
  await a.click('#log .acts button:text("Delete for everyone")');
  for (const p of [b, d]) await p.waitForSelector('#log li.deleted:has-text("Removed by the room owner")', { timeout: 8000 });
  await c.waitForSelector('#log li.me.deleted', { timeout: 8000 });
  check('owner deletes a member’s message for everyone', true);
  await a.selectOption('#s-chat-ttl', '60');
  await b.waitForFunction(() => /owner set messages to disappear/.test(document.querySelector('#log')?.textContent), null, { timeout: 8000 });
  check('only the owner sets the timer', await b.isDisabled('#s-chat-ttl'));

  // ---- removal, leaving, closing ----
  a.once('dialog', (dl) => dl.accept());
  await a.click('#members li:has-text("Carol") button:text("Remove")');
  await c.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  check('removed member is told', /removed from the room/.test(await c.textContent('#note-text')));
  await b.waitForFunction(() => document.querySelectorAll('#members li').length === 3, null, { timeout: 10000 });
  check('members see the new member list', !/Carol/.test((await rows(b)).join()));

  await b.click('#b-leave');
  await a.waitForFunction(() => /Bob.*is no longer in the room/.test(document.querySelector('#log')?.textContent), null, { timeout: 10000 });
  await d.waitForFunction(() => document.querySelectorAll('#members li').length === 2, null, { timeout: 10000 });
  check('a member leaves; the others update', true);

  a.once('dialog', (dl) => dl.accept());
  await a.click('#b-leave');
  await d.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  check('owner closes the room for everyone', (await d.textContent('#note-title')) === 'Room closed', await d.textContent('#note-text'));

  check('no CSP violations or page errors', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('room flow', false, e.message.split('\n')[0]);
  for (const [who, p] of Object.entries(pages)) {
    const st = await p.evaluate(() => [document.querySelector('.view:not([hidden])')?.id, document.querySelector('#status')?.textContent,
      document.querySelector('#error').hidden ? '' : document.querySelector('#error')?.textContent,
      [...document.querySelectorAll('#members li')].map((l) => l.textContent).join(' | '),
      [...document.querySelectorAll('#log li.sys')].map((l) => l.textContent).slice(-3).join(' | ')].join(' · ')).catch((x) => x.message);
    console.log(`  ${who}: ${st}`);
  }
  if (problems.length) console.log(problems.join('\n'));
} finally {
  await browser.close();
  srv.close();
}
finish();
