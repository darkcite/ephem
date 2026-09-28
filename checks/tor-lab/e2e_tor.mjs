// E4 browser half (docs/P2P-CHAT.md Appendix C.5): Chromium loads tor_bg.wasm, bootstraps
// arti through the offline lab (Go broker → WebRTC → Go proxy → Go snowflake server → bridge)
// and echoes through the lab's onion service. Needs `checks/tor-lab/lab.sh up` and
// `checks/tor-lab/build_web.sh`.
import * as fs from 'node:fs';
import { check, finish, launch, problems, serve, watch } from '../e2e_lib.mjs';

const env = Object.fromEntries(fs.readFileSync('/tmp/ephlab/lab.env', 'utf8').trim().split('\n').map((l) => l.split('=')));
const net = fs.readFileSync(`${env.LAB}/arti-net.toml`, 'utf8');
const srv = await serve();
const browser = await launch();
try {
  const page = await (await browser.newContext()).newPage();
  watch(page, 'tor');
  page.on('console', (m) => { if (process.env.VERBOSE) console.log('  |', m.text()); });
  await page.goto(`http://127.0.0.1:${srv.address().port}/checks/tor-lab/web/index.html`);
  await page.waitForFunction(() => typeof window.run === 'function');
  const r = await page.evaluate((a) => window.run(a), {
    broker: env.BROKER_URL, fp: env.BRIDGE_FP, stun: env.STUN_URL, nat: 'unrestricted', net, onion: env.ONION, port: Number(env.ONION_PORT),
    level: process.env.TOR_LOG || 'info',
  });
  for (const s of r.steps) console.log('  ' + s);
  check('arti in the browser bootstraps through Snowflake (lab)', !!r.bootstrapMs, r.error || `${r.bootstrapMs} ms`);
  check('browser reaches an onion service and gets the echo', r.ok === true, r.error || `connected ${r.connectMs} ms, echo ${r.echoMs} ms`);

  // E5: a tab hosts an onion service; another browser (own arti, own Snowflake) dials it.
  const args = { broker: env.BROKER_URL, fp: env.BRIDGE_FP, stun: env.STUN_URL, nat: 'unrestricted', net, level: process.env.TOR_LOG || 'info' };
  const url = `http://127.0.0.1:${srv.address().port}/checks/tor-lab/web/index.html`;
  const [ha, hb] = [await (await browser.newContext()).newPage(), await (await browser.newContext()).newPage()];
  for (const [p, w] of [[ha, 'host'], [hb, 'dialer']]) {
    watch(p, w);
    p.on('console', (m) => { if (process.env.VERBOSE) console.log(`  ${w} |`, m.text()); });
    await p.goto(url);
    await p.waitForFunction(() => typeof window.hostTab === 'function');
  }
  const h = await ha.evaluate((a) => window.hostTab(a), args);
  check('a browser tab hosts an onion service', /^[a-z2-7]{56}\.onion$/.test(h.onion || ''), h.onion);
  const d = await hb.evaluate(([a, o]) => window.dialTab(a, o), [args, h.onion]);
  for (const s of d.steps || []) console.log('  ' + s);
  const served = await ha.evaluate(() => window.served);
  check('another browser reaches it and they talk both ways', d.reply === 'pong' && served === 'ping', d.error || `${d.ms} ms`);
  check('no page errors', problems.filter((p) => /pageerror/.test(p)).length === 0, problems.join(' | '));
} catch (e) {
  check('tor lab flow', false, e.message.split('\n')[0]);
} finally {
  await browser.close();
  srv.close();
}
finish();
