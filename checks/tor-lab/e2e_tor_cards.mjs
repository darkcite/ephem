// Contact cards over Tor (docs/P2P-CHAT.md §7.5, §28.4 case 3), in the offline lab:
//
//   Alice shows her card → Bob and Carol add her from it → Bob presses Connect: the dial carries
//   the card's secret, Alice is asked "… (from your contact card) wants to connect" and sees no
//   chat before she accepts → accepted: both are contacts, messages both ways → Bob leaves, Alice
//   resets her card → Carol's old card no longer opens a chat → Bob connects again as a plain
//   contact (his card secret was dropped after the first connection).
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 for the real Tor network.
import { PASS, check, finish, launch, msgWith, problems, watch } from '../e2e_lib.mjs';
import { T, dumpLogs, record, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];

async function open(who, nick) {
  const b = await launch();
  browsers.push(b);
  const ctx = await torContext(await b.newContext({ acceptDownloads: true }));
  const p = await ctx.newPage();
  watch(p, who);
  record(p, who);
  p.answers = [];
  p.on('dialog', (d) => {
    const a = p.answers.shift();
    if (a === false) d.dismiss();
    else d.accept(typeof a === 'string' ? a : undefined);
  });
  await p.goto(`${base}/tor.html`);
  await p.fill('#i-nick', nick);
  await p.dispatchEvent('#i-nick', 'change');
  await p.click('#b-id-save');
  await p.fill('#i-label', who);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
  await torReady(p, who);
  return p;
}

const connectTo = (p, name) => p.locator('#contacts li', { hasText: name }).locator('button', { hasText: 'Connect' }).click();

try {
  const [a, b, c] = await Promise.all([open('alice', 'Alice'), open('bob', 'Bob'), open('carol', 'Carol')]);
  await a.click('#b-card');
  await a.waitForFunction(() => document.querySelector('#card .link').value.includes('#k='));
  const card = await a.inputValue('#card .link');
  for (const p of [b, c]) {
    p.answers.push('Alice');
    await p.fill('#t-code', card);
    await p.click('#b-apply');
    await p.locator('#contacts li', { hasText: 'Alice' }).waitFor({ timeout: 10_000 });
  }
  check('Bob and Carol add Alice from her card (Connect offered)', await b.locator('#contacts li', { hasText: 'Alice' }).locator('button', { hasText: 'Connect' }).isVisible());

  const t1 = Date.now();
  await connectTo(b, 'Alice');
  await a.waitForSelector('#card-req:not([hidden])', { timeout: T });
  check('Alice is asked first; no chat shown before she accepts', /from your contact card/.test(await a.textContent('#card-req-text')) && await a.isHidden('#log') && await a.isHidden('#f-send'), `${Date.now() - t1} ms`);
  await a.click('#b-card-accept');
  await a.locator('#contacts li', { hasText: 'Bob' }).waitFor({ state: 'attached', timeout: T });
  await b.fill('#t-msg', 'hello via your card');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'hello via your card').waitFor({ timeout: 60_000 });
  await a.fill('#t-msg', 'welcome');
  await a.press('#t-msg', 'Enter');
  await msgWith(b, 'them', 'welcome').waitFor({ timeout: 60_000 });
  check('accepted: Bob is Alice\'s contact, messages both ways', true);

  b.answers.push(true);
  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: T });
  await Promise.all([a.click('#b-again'), b.click('#b-again')]);
  a.answers.push(true);
  if (await a.isHidden('#card')) await a.click('#b-card');
  await a.click('#b-card-reset');

  // Carol's card is now stale: her dial is dropped unanswered.
  await connectTo(c, 'Alice');
  const stale = await a.waitForSelector('#card-req:not([hidden]), #v-chat:not([hidden])', { timeout: 60_000 }).then(() => false, () => true);
  check('after a reset, an old card opens nothing', stale);
  await c.click('#b-again');

  // Bob is a contact now: a plain contact dial (no card secret, no prompt).
  const t2 = Date.now();
  await connectTo(b, 'Alice');
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: T })));
  check('Bob reconnects as a contact: no prompt this time', await a.isHidden('#card-req'), `${Date.now() - t2} ms`);
  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('tor cards flow', false, e.message.split('\n')[0]);
  dumpLogs();
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
