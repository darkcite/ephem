// TOR-2 (docs/P2P-CHAT.md §28, Appendix C.5): the Ephem app in Tor mode, end to end, in the
// offline lab. Two separate browsers open app/tor.html; each runs its own arti over Snowflake
// (lab broker → WebRTC → Go proxy → Go snowflake server → bridge) and hosts its onion service.
// Alice creates a TOR_INVITE (single code, no answer); Bob opens it, dials her onion, the Noise
// IK handshake runs over the Tor stream, both see the same SAS, and they chat both ways.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`; LIVE=1 runs it on the real Tor network
// instead (tor_env.mjs).
import { PASS, check, finish, launch, msgWith, problems, watch } from '../e2e_lib.mjs';
import { T, serveTor, torContext } from './tor_env.mjs';

const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];
const logs = []; // console of every page, printed if the flow fails

/** Saves the tab's identity (contacts need one, §7.5); returns the key text. */
async function saveIdentity(p, label) {
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
  return p.inputValue('#t-keytext');
}

async function open(who) {
  const b = await launch();
  browsers.push(b);
  const ctx = await b.newContext({ acceptDownloads: true });
  await torContext(ctx);
  const p = await ctx.newPage();
  watch(p, who);
  p.on('dialog', (d) => d.accept(who === 'alice' ? 'Bob' : 'Alice'));
  p.on('console', (m) => {
    logs.push(`  ${who} | ${m.text()}`);
    if (process.env.VERBOSE) console.log(`  ${who} |`, m.text());
  });
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
  check('tor.html runs the Tor build', await a.evaluate(() => document.documentElement.dataset.mode === 'tor' && !document.querySelector('#b-id-receive').offsetParent));
  await a.waitForFunction(() => /Reachable through Tor/.test(document.querySelector('#tor-state').textContent), null, { timeout: T });
  check('Alice: Tor up and her onion service hosted', true, `${Date.now() - t0} ms`);

  // Both sign in with saved identities (for TOR-3 below). Alice also signs out and back in:
  // her onion service follows the identity (hosted again).
  const keyA = await saveIdentity(a, 'A');
  await saveIdentity(b, 'B');
  const handleA = await a.textContent('#me');
  await a.click('#b-id-temp');
  await a.click('#b-id-load');
  await a.fill('#t-keyin', keyA);
  await a.fill('#i-pass-in', PASS);
  await a.click('#b-id-do-load');
  await a.waitForFunction((h) => document.querySelector('#me').textContent === h, handleA, { timeout: 10_000 });
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

  // TOR-3 (§28.7): contacts store each other's onion; later Bob dials Alice without a code.
  await a.click('#b-save-contact');
  await b.click('#b-save-contact');
  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: T });
  await b.click('#b-again');
  const connect = b.locator('#contacts li', { hasText: 'Alice' }).locator('button', { hasText: 'Connect' });
  check('contact saved from a Tor chat offers "Connect"', await connect.isVisible());
  const t3 = Date.now();
  await connect.click();
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: T })));
  await b.fill('#t-msg', 'called you');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'called you').waitFor({ timeout: 60_000 });
  check('Bob connects to contact Alice through Tor, no code', true, `${Date.now() - t3} ms`);

  // Warm start (§28.3): the directory snapshot is in IndexedDB; a reload bootstraps from it.
  const snap = await a.evaluate(() => new Promise((res) => {
    const r = indexedDB.open('ephem-tor', 1);
    r.onsuccess = () => {
      const g = r.result.transaction('dir').objectStore('dir').getAll();
      g.onsuccess = () => res(g.result.map((v) => v.length));
    };
    r.onerror = () => res([]);
  }));
  check('Tor directory snapshot kept in IndexedDB', snap.length === 1 && snap[0] > 1000, `${Math.round((snap[0] || 0) / 1024)} KB`);
  const t4 = Date.now();
  await a.reload();
  await a.waitForFunction(() => /Reachable through Tor/.test(document.querySelector('#tor-state').textContent), null, { timeout: T });
  check('reload: warm start from the snapshot', true, `${Date.now() - t4} ms`);

  check('no reconnect-code UI in Tor mode', await a.isHidden('#resume') && await b.isHidden('#resume'));
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('tor app flow', false, e.message.split('\n')[0]);
  for (const b of browsers) for (const c of b.contexts()) for (const p of c.pages()) {
    console.log('  tor state:', await p.textContent('#tor-state').catch(() => '?'), '| status:', await p.textContent('#status').catch(() => '?'));
  }
  if (!process.env.VERBOSE) console.log(logs.slice(-60).join('\n'));
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
