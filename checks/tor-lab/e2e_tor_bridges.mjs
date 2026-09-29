// BR-2 (docs/P2P-CHAT.md Appendix F.2): the user's own Tor bridges, end to end in the offline
// lab. The lab's Snowflake is pasted as bridge lines in the settings, the way a user would paste
// lines from whoever runs a bridge; Tor then starts with them, they are kept in the encrypted key
// file, a reload waits for them until sign-in, and they travel as a `#b=` link.
//
// Needs `checks/tor-lab/lab.sh up` and `./build.sh`. Lab only (LIVE has no bridges of ours).
import { PASS, check, finish, launch, problems, watch } from '../e2e_lib.mjs';
import { LIVE, dumpLogs, labBridges, record, serveTor, torContext, torReady, unexpected } from './tor_env.mjs';

if (LIVE) {
  console.log('e2e_tor_bridges: lab only, skipped with LIVE=1');
  process.exit(0);
}

const srv = await serveTor();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browsers = [];
const OBFS4 = 'obfs4 198.51.100.1:443 0123456789ABCDEF0123456789ABCDEF01234567 cert=x iat-mode=0';

async function open(who, extra, wait, hash = '') {
  const b = await launch();
  browsers.push(b);
  const ctx = await b.newContext({ acceptDownloads: true });
  await torContext(ctx, extra);
  // "My own bridges" was chosen in this browser before: Tor waits for them.
  if (wait) await ctx.addInitScript(() => localStorage.setItem('ephem-bridges', '1'));
  const p = await ctx.newPage();
  watch(p, who);
  p.on('dialog', (d) => d.accept());
  record(p, who);
  await p.goto(`${base}/tor.html${hash}`);
  return p;
}

async function useBridges(p, lines) {
  await p.evaluate(() => { document.querySelector('#settings').open = true; });
  await p.check('#r-br-custom');
  await p.fill('#t-bridges', lines);
  await p.click('#b-br-apply');
}

try {
  // A bridge link opened on the direct page moves to Tor mode (§28.2).
  {
    const b = await launch();
    browsers.push(b);
    const p = await b.newPage();
    await p.goto(`${base}/#b=AAAA`);
    await p.waitForURL(/\/app\/tor\.html/, { timeout: 10_000 });
    check('a bridge link opens in Tor mode', true);
    await b.close();
    browsers.pop();
  }

  const lines = labBridges();
  const a = await open('alice', { bridges: null }, true);
  await a.waitForFunction(() => /waiting for your own bridges/.test(document.querySelector('#tor-state').textContent), null, { timeout: 20_000 });
  check('with "my own bridges" chosen, Tor waits for them', await a.evaluate(() => !/Snowflake:/.test(document.querySelector('#tor-state').textContent)));

  await useBridges(a, OBFS4);
  await a.waitForSelector('#br-problems:not([hidden])');
  const why = await a.textContent('#br-problems');
  check('an obfs4 line is refused with its reason; Tor does not start', /Line 1: not used: obfs4 needs a direct TCP connection/.test(why) && /No usable snowflake bridge/.test(why), why.slice(0, 90));

  const t0 = Date.now();
  await useBridges(a, `${OBFS4}\n${lines}`);
  await torReady(a, 'alice');
  check('the lab Snowflake pasted as bridge lines: Tor up, onion hosted', /your bridges/.test(await a.textContent('#br-state')), `${Date.now() - t0} ms`);

  await a.click('#b-br-share');
  const link = await a.inputValue('#br-link');
  const shared = Buffer.from(link.split('#b=')[1], 'base64url').toString('utf8');
  check('"Share my bridges" gives a #b= link with the lines', shared === `${OBFS4}\n${lines}`, `${link.length} chars`);

  // Kept in the key file: a reload waits, signing in starts Tor with them.
  await a.click('#b-id-save');
  await a.fill('#i-label', 'A');
  await a.fill('#i-pass', PASS);
  await a.fill('#i-pass2', PASS);
  await Promise.all([a.waitForEvent('download'), a.click('#b-id-do-save')]);
  const key = await a.inputValue('#t-keytext');
  await a.reload();
  await a.waitForFunction(() => /waiting for your own bridges/.test(document.querySelector('#tor-state').textContent), null, { timeout: 20_000 });
  await a.click('#b-id-load');
  await a.fill('#t-keyin', key);
  await a.fill('#i-pass-in', PASS);
  await a.click('#b-id-do-load');
  const t1 = Date.now();
  await torReady(a, 'alice');
  check('after a reload, signing in starts Tor with the bridges from the key file', /your bridges/.test(await a.textContent('#br-state')), `${Date.now() - t1} ms`);

  // Bob opens the link: the setting is filled, not applied (his Tor keeps the lab setup).
  const b = await open('bob', {}, false, link.slice(link.indexOf('#')));
  await b.waitForFunction(() => document.querySelector('#t-bridges').value.length > 0, null, { timeout: 20_000 });
  check('Bob opens the link: settings filled, not applied', await b.isChecked('#r-br-custom') && /check them/.test(await b.textContent('#br-state')) && (await b.inputValue('#t-bridges')) === `${OBFS4}\n${lines}`);
  await torReady(b, 'bob');

  // And the Tor that started from pasted lines carries a chat.
  await a.click('#b-invite');
  await a.waitForFunction(() => document.querySelector('#v-code .link').value.includes('#t='), null, { timeout: 10_000 });
  await b.fill('#t-code', await a.inputValue('#v-code .link'));
  await b.click('#b-apply');
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: 120_000 })));
  check('a chat through Alice\'s own bridges', true);

  check('no page errors or CSP violations', unexpected(problems).length === 0, unexpected(problems).join(' | '));
} catch (e) {
  check('bridges flow', false, e.message.split('\n')[0]);
  dumpLogs(60);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
