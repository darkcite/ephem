// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// APP-E2E-MULTI (docs/P2P-CHAT.md Appendix F.3, UI-2) in real Chromium, direct mode: one tab
// holds several chats at once. Alice chats with Bob and with Carol (two invites); each chat
// keeps its own messages, receipts and draft; a message in the chat that is not on screen raises
// an unread count; Bob leaving ends only his chat; a phone-sized window shows the list, then the
// chat with a Back button; the tabs and lists carry their ARIA roles.
//
// Usage: node checks/e2e_multi.mjs   (build first with ./build.sh)
import { check, connect, finish, launch, msgWith, problems, serve, toSettings, watch } from './e2e_lib.mjs';

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();

async function open(who, viewport) {
  const ctx = await browser.newContext(viewport ? { viewport } : {});
  const p = await ctx.newPage();
  watch(p, who);
  p.on('dialog', (d) => d.accept());
  await p.goto(`${base}/app/`);
  // A phone starts on the list; wider windows show the New chat pane beside it.
  await p.waitForSelector(viewport ? '#b-new' : '#v-start:not([hidden])');
  await p.waitForFunction(() => document.querySelector('#me')?.textContent !== '…');
  return p;
}

const say = async (p, t) => {
  await p.fill('#t-msg', t);
  await p.press('#t-msg', 'Enter');
};
const rows = (p) => p.$$eval('#chats li', (ls) => ls.map((l) => l.textContent));
const pick = (p, text) => p.locator('#chats li', { hasText: text }).click();

try {
  const [a, b, c] = await Promise.all([open('alice'), open('bob'), open('carol')]);
  for (const [p, n] of [[a, 'Alice'], [b, 'Bob'], [c, 'Carol']]) {
    await toSettings(p);
    await p.fill('#i-nick', n);
    await p.dispatchEvent('#i-nick', 'change');
  }

  // Alice ↔ Bob, then (from the list's "New chat") Alice ↔ Carol, both open at once.
  await connect(a, b);
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: 20_000 })));
  await say(a, 'hi Bob');
  await msgWith(b, 'them', 'hi Bob').waitFor({ timeout: 10_000 });
  await a.fill('#t-msg', 'draft for Bob');
  await a.click('#b-new');
  await connect(a, c);
  await Promise.all([a, c].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: 20_000 })));
  await a.waitForFunction(() => document.querySelectorAll('#chats li').length === 2);
  const list = await rows(a);
  check('two chats open in one tab, both in the list', list.some((r) => /Bob/.test(r)) && list.some((r) => /Carol/.test(r)), list.join(' | '));
  await say(a, 'hi Carol');
  await msgWith(c, 'them', 'hi Carol').waitFor({ timeout: 10_000 });
  check('each chat keeps its own messages', !(await a.textContent('#log')).includes('hi Bob') && (await a.textContent('#log')).includes('hi Carol'));

  // A message in the chat that is not on screen: unread count in the list and on the tab.
  await say(b, 'are you there?');
  await a.waitForFunction(() => /Bob.*1/.test([...document.querySelectorAll('#chats li')].map((l) => l.textContent).join('|')), null, { timeout: 10_000 });
  check('a message in the background chat raises its unread count', await a.isVisible('#badge-chats') && (await a.textContent('#badge-chats')) === '1');
  const note = a.locator('#notices .notice', { hasText: 'Bob' });
  await say(b, 'hello again');
  await a.waitForFunction(() => /2 new messages/.test(document.querySelector('#notices')?.textContent), null, { timeout: 10_000 });
  check('an in-app notice names the other chat, one per chat (a burst updates its count), without the text',
    await note.count() === 1 && !(await a.textContent('#notices')).includes('are you there'), (await note.textContent()).trim());
  await note.click();
  await a.waitForFunction(() => document.querySelector('#log')?.textContent.includes('are you there?'));
  check('tapping the notice opens that chat', await a.locator('#notices .notice').count() === 0);
  check('switching chats: its messages, its draft; unread cleared', (await a.inputValue('#t-msg')) === 'draft for Bob' && await a.isHidden('#badge-chats'));
  await a.fill('#t-msg', '');
  await say(a, 'yes, Bob');
  await b.waitForFunction(() => document.querySelector('#log li.me .tick')?.textContent === '✓✓', null, { timeout: 10_000 });
  check('receipts per chat (Bob sees ✓✓ on his own message)', true);

  // Bob leaves: only his chat ends; Carol's goes on.
  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: 20_000 });
  await a.click('#b-again');
  await a.waitForFunction(() => document.querySelectorAll('#chats li').length === 1);
  await pick(a, 'Carol');
  await say(c, 'still here');
  await msgWith(a, 'them', 'still here').waitFor({ timeout: 10_000 });
  check('Bob leaving ends only his chat; Carol\'s continues', (await rows(a)).length === 1);

  // Performance readout: delivery latency of Carol's chat, main-thread load, WebAssembly memory.
  await say(a, 'timing this');
  await msgWith(c, 'them', 'timing this').waitFor({ timeout: 10_000 });
  await a.waitForFunction(() => [...document.querySelectorAll('#log li.me .tick')].at(-1)?.textContent.includes('✓'), null, { timeout: 10_000 });
  await toSettings(a);
  await a.click('#perf summary');
  await a.waitForFunction(() => /delivered in \d+ ms/.test(document.querySelector('#perf-text')?.textContent), null, { timeout: 5_000 });
  const perfText = await a.textContent('#perf-text');
  await a.check('#c-hud');
  await a.waitForFunction(() => /CPU ~\d+%/.test(document.querySelector('#hud')?.textContent), null, { timeout: 3_000 });
  check('performance readout: delivery latency per chat, main-thread load, wasm memory; the overlay shows them',
    /CPU \(main\)\s+~[\d.]+ % busy/.test(perfText) && /wasm \(app\)\s+[\d.]+ MB/.test(perfText), (await a.textContent('#hud')).trim());
  await a.uncheck('#c-hud');
  await a.click('#tab-chats');
  await pick(a, 'Carol');

  // ARIA: tabs and lists (a screen reader finds its way).
  const roles = await a.evaluate(() => ({
    tablist: document.querySelectorAll('[role=tablist] [role=tab]').length,
    selected: document.querySelector('[role=tab][aria-selected=true]')?.id,
    panels: document.querySelectorAll('[role=tabpanel]').length,
    live: document.querySelector('#log')?.getAttribute('aria-live'),
  }));
  check('tabs, tab panels and the live message log carry their ARIA roles', roles.tablist === 4 && roles.selected === 'tab-chats' && roles.panels === 4 && roles.live === 'polite', JSON.stringify(roles));

  // "Got a code?" from every tab: the header's Code button.
  await a.click('#tab-settings');
  await a.click('#b-code');
  const sheet = await a.isVisible('#t-code') && await a.isVisible('#b-scan');
  await a.keyboard.press('Escape');
  check('the Code button opens "Got a code?" from any tab (here Settings); Escape closes it', sheet && await a.isHidden('#code-sheet'));
  await a.click('#tab-chats');

  // Phone-sized window: the list first; a chat opens full screen with Back.
  const ph = await open('phone', { width: 390, height: 800 });
  check('phone: the list (and the tab bar) first, no pane', await ph.isVisible('#b-new') && await ph.isVisible('#tab-follow') && await ph.isHidden('#pane'));
  await ph.click('#b-new');
  check('phone: “New chat” opens the pane full screen, with Back; the tab bar stays', await ph.isVisible('#b-invite') && await ph.isHidden('#side') && await ph.isVisible('#b-back') && await ph.isVisible('#tab-settings'));
  await ph.click('#tab-settings');
  check('phone: Settings is a tab of its own (identity, contacts, connection), no Back; the tab bar stays', await ph.isVisible('#b-id-save') && await ph.isVisible('#s-ttl') && await ph.isVisible('#tab-chats') && await ph.isHidden('#b-back'));
  const frame = await ph.evaluate(() => ({ page: document.scrollingElement.scrollHeight - innerHeight, pane: document.querySelector('#pane').scrollHeight > document.querySelector('#pane').clientHeight }));
  check('phone: the page itself never scrolls (full screen), only the pane', frame.page <= 0 && frame.pane, JSON.stringify(frame));
  check('phone: the Code button is in the header', await ph.isVisible('#b-code'));
  await ph.click('#tab-chats');
  check('phone: a tab opens on its list (Chats: the chats)', await ph.isVisible('#b-new') && await ph.isHidden('#pane'));
  await ph.click('#b-new');
  await ph.click('#b-back');
  check('phone: Back returns to the list', await ph.isVisible('#b-new') && await ph.isHidden('#pane'));
  await ph.click('#tab-follow');
  check('phone: Following opens on its list', await ph.isVisible('#b-follow-new') && await ph.isHidden('#pane'));
  await ph.click('#tab-chats');
  const overflow = await ph.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  check('phone: no horizontal scrolling', !overflow);
  // Settings with both debug readouts open: nothing wider than the screen (a long readout line
  // once widened the pane and the page scrolled sideways).
  await ph.click('#tab-settings');
  await ph.click('#perf summary');
  await ph.click('#disp summary');
  await ph.waitForTimeout(1200);
  const wide = await ph.evaluate(() => ({ page: document.documentElement.scrollWidth - innerWidth, pane: document.querySelector('#pane').scrollWidth - document.querySelector('#pane').clientWidth }));
  check('phone: debug readouts open in Settings, still no sideways scrolling', wide.page <= 0 && wide.pane <= 0, JSON.stringify(wide));
  await ph.click('#tab-chats');

  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('multi-chat flow', false, e.message.split('\n')[0]);
} finally {
  await browser.close();
  srv.close();
}
finish();
