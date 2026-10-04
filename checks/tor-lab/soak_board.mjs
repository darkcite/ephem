// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// B-P11 from the build container (docs/BOARDS.md G.16.2): the probe page
// (checks/spikes/board_soak/probe.html) reads a board every 5 minutes through the real Tor network
// (RELAY=1, `lab.sh relay`), for SOAK_H hours; its report is written to SOAK_OUT every minute.
//   RELAY=1 SOAK_LINK='https://…/app/tor.html#B=…&o=….onion' node checks/tor-lab/soak_board.mjs
import * as fs from 'node:fs';
import { launch, serve } from '../e2e_lib.mjs';
import { RELAY, torContext } from './tor_env.mjs';

const LINK = process.env.SOAK_LINK;
const HOURS = Number(process.env.SOAK_H || 24);
const OUT = process.env.SOAK_OUT || '/tmp/soak.txt';
if (!LINK || !RELAY) throw new Error('needs RELAY=1 and SOAK_LINK');
const broker = new URL(JSON.parse(JSON.stringify(Object.fromEntries(fs.readFileSync('/tmp/ephrelay/relay.env', 'utf8').trim().split('\n').map((l) => l.split('='))))).BROKER_URL).origin;
// The probe page's CSP, plus the local relay broker (what serveTor does for tor.html).
const srv = await serve((p, read) => (p.endsWith('/board_soak/probe.html') ? read().replace("connect-src 'self' https:", `connect-src 'self' https: ${broker}`) : null));
const b = await launch();
const page = await (await torContext(await b.newContext())).newPage();
// SOAK_VERBOSE=1 keeps every console line (arti's log), to see where a slow read waits.
const VERBOSE = process.env.SOAK_VERBOSE === '1';
page.on('console', (m) => { if (VERBOSE || /error|panic/i.test(m.text())) fs.appendFileSync(`${OUT}.log`, `${new Date().toISOString()} ${m.text()}\n`); });
await page.goto(`http://127.0.0.1:${srv.address().port}/checks/spikes/board_soak/probe.html${VERBOSE ? '?log=debug' : ''}`);
await page.fill('#link', LINK);
await page.click('#start');
const end = Date.now() + HOURS * 3_600_000;
while (Date.now() < end) {
  await new Promise((r) => setTimeout(r, 60_000));
  try { fs.writeFileSync(OUT, await page.textContent('#out')); } catch (e) { fs.appendFileSync(`${OUT}.log`, `${new Date().toISOString()} ${e.message}\n`); }
}
fs.writeFileSync(OUT, `${await page.textContent('#out')}\n(finished)`);
await b.close();
srv.close();
