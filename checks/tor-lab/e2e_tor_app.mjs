// TOR-2 (docs/P2P-CHAT.md §28, Appendix C.5): the Ephem app in Tor mode, end to end, in the
// offline lab. Two separate browsers open app/tor.html; each runs its own arti over Snowflake
// (lab broker → WebRTC → Go proxy → Go snowflake server → bridge) and hosts its onion service.
// Alice creates a TOR_INVITE (single code, no answer); Bob opens it, dials her onion, the Noise
// IK handshake runs over the Tor stream, both see the same SAS, and they chat both ways.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`. The page is served unchanged except that
// its CSP names the lab broker instead of the real one, and the lab settings are injected as
// `ephemTorLab` before the page loads (app.js `startTor`).
import * as fs from 'node:fs';
import { check, finish, launch, msgWith, problems, serve, watch } from '../e2e_lib.mjs';

const env = Object.fromEntries(fs.readFileSync('/tmp/ephlab/lab.env', 'utf8').trim().split('\n').map((l) => l.split('=')));
const lab = {
  broker: env.BROKER_URL,
  fingerprint: env.BRIDGE_FP,
  ice: env.STUN_URL,
  nat: 'unrestricted',
  network: fs.readFileSync(`${env.LAB}/arti-net.toml`, 'utf8'),
  log: process.env.TOR_LOG || '',
};
const origin = new URL(env.BROKER_URL).origin;
const srv = await serve((p, read) => (p.endsWith('/tor.html') ? read().replace('https://snowflake-broker.torproject.net', origin) : null));
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];
const T = 180_000;

async function open(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await b.newContext();
  await ctx.addInitScript((c) => { globalThis.ephemTorLab = c; }, lab);
  const p = await ctx.newPage();
  watch(p, who);
  p.on('console', (m) => { if (process.env.VERBOSE) console.log(`  ${who} |`, m.text()); });
  await p.goto(`${base}/tor.html`);
  return p;
}

try {
  // Mode routing (§28.2): a Tor invite opened on the direct page moves to tor.html, and back.
  {
    const b = await launch();
    browsers.push(b);
    const p = await b.newPage();
    await p.goto(`${base}/#t=AAAA`);
    await p.waitForURL(/\/app\/tor\.html/, { timeout: 10_000 });
    check('a Tor invite link opens in Tor mode', true);
    const q = await b.newPage(); // a fresh document (a fragment change alone does not reload)
    await q.goto(`${base}/tor.html#i=AAAA`);
    await q.waitForURL(/\/app\/(#.*)?$/, { timeout: 10_000 });
    check('a direct invite link opens in direct mode', true);
    await b.close();
    browsers.pop();
  }

  const t0 = Date.now();
  const [a, b] = await Promise.all([open('alice'), open('bob')]);
  check('tor.html runs the Tor build', await a.evaluate(() => document.documentElement.dataset.mode === 'tor' && !document.querySelector('#b-room').offsetParent));
  await a.waitForFunction(() => /Reachable through Tor/.test(document.querySelector('#tor-state').textContent), null, { timeout: T });
  check('Alice: Tor up and her onion service hosted', true, `${Date.now() - t0} ms`);

  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link').value.includes('#t='), null, { timeout: 10_000 });
  const link = await a.inputValue('#v-code .link');
  const code = link.split('#t=')[1];
  check('Alice: one Tor invite (kind 5, 104 bytes), no answer box', Buffer.from(code, 'base64url').length === 104 && await a.isHidden('#answer-box'), `${code.length} chars`);

  const t1 = Date.now();
  await b.fill('#t-code', link);
  await b.click('#b-apply');
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: T })));
  check('Bob dials Alice\'s onion: both chats open', true, `${Date.now() - t1} ms`);
  const [sa, sb] = [await a.textContent('#sas-digits'), await b.textContent('#sas-digits')];
  check('same safety code on both sides', sa === sb && /^\d{3} \d{3}$/.test(sa), sa);
  check('chat says "through Tor"', /through Tor/.test(await a.textContent('#log')) && /through Tor/.test(await b.textContent('#log')));

  const t2 = Date.now();
  await a.fill('#t-msg', 'hello over tor');
  await a.press('#t-msg', 'Enter');
  await msgWith(b, 'them', 'hello over tor').waitFor({ timeout: 60_000 });
  const ms1 = Date.now() - t2;
  await b.fill('#t-msg', 'hi alice');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'hi alice').waitFor({ timeout: 60_000 });
  check('messages both ways', true, `A→B ${ms1} ms, B→A ${Date.now() - t2 - ms1} ms`);
  await a.locator('#log li.me .tick.ok').first().waitFor({ timeout: 60_000 }).then(() => check('delivery receipt', true), () => check('delivery receipt', false));

  // Stream loss (§28.5): no codes; whoever dialled (Bob) dials the onion again, the queue resends.
  for (const [who, p, other] of [['Bob (dialler)', b, a], ['Alice (host)', a, b]]) {
    const t = Date.now();
    const before = await p.locator('#log li', { hasText: 'Reconnected through Tor' }).count();
    await p.click('#b-info');
    await p.click('#b-drop');
    await p.click('#b-info');
    const text = `after ${who} lost the stream`;
    await p.fill('#t-msg', text);
    await p.press('#t-msg', 'Enter');
    await msgWith(other, 'them', text).waitFor({ timeout: T });
    const again = await p.locator('#log li', { hasText: 'Reconnected through Tor' }).count();
    check(`${who} loses the stream: redial, queued message delivered`, again > before, `${Date.now() - t} ms`);
  }

  // No chat traffic outside Tor: the only RTCPeerConnections are Snowflake's (to the lab proxy).
  check('no reconnect-code UI in Tor mode', await a.isHidden('#resume') && await b.isHidden('#resume'));
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('tor app flow', false, e.message.split('\n')[0]);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
