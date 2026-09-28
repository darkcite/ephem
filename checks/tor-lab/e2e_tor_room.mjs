// TOR-4 (docs/P2P-CHAT.md §28.7): a room in Tor mode, in the offline lab. Three browsers, each
// with its own arti over Snowflake and its own onion service: owner A invites B and C with
// TOR_INVITEs; the owner introduces B and C (sealed TOR_INVITE relayed through it) and one of
// them dials the other's onion, so the mesh is complete without anyone seeing an IP address.
// Then room messages, and a member losing its member links (the dialler redials).
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh` (as e2e_tor_app.mjs).
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

async function open(who, nick) {
  const b = await launch();
  browsers.push(b);
  const ctx = await b.newContext();
  await ctx.addInitScript((c) => { globalThis.ephemTorLab = c; }, lab);
  const p = await ctx.newPage();
  watch(p, who);
  p.on('dialog', (d) => d.accept());
  p.on('console', (m) => { if (process.env.VERBOSE) console.log(`  ${who} |`, m.text()); });
  await p.goto(`${base}/tor.html`);
  await p.fill('#i-nick', nick);
  await p.dispatchEvent('#i-nick', 'change');
  await p.waitForFunction(() => /Reachable through Tor/.test(document.querySelector('#tor-state').textContent), null, { timeout: T });
  return p;
}

/** Every row of `page`'s member list is itself or linked via Tor, and there are `n` rows. */
const mesh = (page, n) => page.waitForFunction((n) => {
  const ls = [...document.querySelectorAll('#members li')];
  return ls.length === n && ls.every((l) => /\(you\)|via Tor/.test(l.textContent));
}, n, { timeout: T });

/** Owner `a` invites `p` with a Tor invite; resolves when `p` is in the room. */
async function join(a, p) {
  await a.click('#b-room-invite');
  await a.waitForFunction(() => document.querySelector('#room-invite .link').value.includes('#t='), null, { timeout: 15_000 });
  check('owner shows a Tor invite, no answer box', await a.isHidden('#t-room-answer'));
  await p.fill('#t-code', await a.inputValue('#room-invite .link'));
  await p.click('#b-apply');
  await p.waitForSelector('#v-chat:not([hidden])', { timeout: T });
  await p.waitForFunction(() => document.querySelectorAll('#members li').length >= 2, null, { timeout: T });
}

try {
  const t0 = Date.now();
  const [a, b, c] = await Promise.all([open('a', 'Alice'), open('b', 'Bob'), open('c', 'Carol')]);
  check('three browsers up on Tor', true, `${Date.now() - t0} ms`);

  await a.click('#b-room');
  await a.waitForSelector('#room:not([hidden])');
  const t1 = Date.now();
  await join(a, b);
  await mesh(a, 2);
  check('B joins the room through Tor', true, `${Date.now() - t1} ms`);
  const t2 = Date.now();
  await join(a, c);
  await Promise.all([mesh(a, 3), mesh(b, 3), mesh(c, 3)]);
  check('C joins; B and C connect onion to onion (introduced by the owner)', true, `${Date.now() - t2} ms`);
  check('no IP-exposure prompt over Tor', await c.isHidden('#room-confirm'));

  await c.fill('#t-msg', 'hi room');
  await c.press('#t-msg', 'Enter');
  await Promise.all([msgWith(a, 'them', 'hi room').waitFor({ timeout: 60_000 }), msgWith(b, 'them', 'hi room').waitFor({ timeout: 60_000 })]);
  check('a member\'s message reaches the owner and the other member', /Carol/.test(await b.locator('#log li.them').last().textContent()));

  // B loses its member links (here: to C); whoever dialled redials, no T2 codes.
  const t3 = Date.now();
  await b.click('#b-info');
  await b.click('#b-drop');
  await b.click('#b-info');
  await b.fill('#t-msg', 'back again');
  await b.press('#t-msg', 'Enter');
  await msgWith(c, 'them', 'back again').waitFor({ timeout: T });
  await Promise.all([mesh(b, 3), mesh(c, 3)]);
  check('member link lost and redialled; queued message delivered', true, `${Date.now() - t3} ms`);
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('tor room flow', false, e.message.split('\n')[0]);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
