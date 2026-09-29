// Shared setup of the Tor-mode e2e tests (e2e_tor_app.mjs, e2e_tor_room.mjs).
//
// Lab (default): the offline lab of `checks/tor-lab/lab.sh up`. tor.html is served with its CSP
// naming the lab broker instead of the real one, and the lab settings are injected as
// `ephemTorLab` before the page loads (app.js `startTor`); nothing else differs.
// LIVE=1: the real Snowflake broker and the real Tor network, the page unchanged (T-10, run on
// a machine that can reach them, e.g. `LIVE=1 E2E_BROWSER=chrome node checks/tor-lab/e2e_tor_app.mjs`).
import * as fs from 'node:fs';
import { serve } from '../e2e_lib.mjs';

export const LIVE = process.env.LIVE === '1';
/** Timeout of Tor steps (bootstrap, first dial): the live network is slower than the lab. */
export const T = LIVE ? 300_000 : 180_000;

const DEAD_BROKER = 'http://127.0.0.1:59999'; // nothing listens: connection refused

const labEnv = () => Object.fromEntries(fs.readFileSync('/tmp/ephlab/lab.env', 'utf8').trim().split('\n').map((l) => l.split('=')));

function labConfig() {
  const env = labEnv();
  return {
    // Bridge lines as a user would paste them (Appendix F.2). A dead broker first: every
    // rendezvous exercises the fallback to the next URL.
    bridges: labBridges(env),
    labBroker: env.BROKER_URL,
    fingerprint: env.BRIDGE_FP,
    nat: 'unrestricted',
    network: fs.readFileSync(`${env.LAB}/arti-net.toml`, 'utf8'),
    log: process.env.TOR_LOG || 'info',
  };
}

/** The lab's Snowflake as bridge lines (`dead`: only the dead broker, unusable in practice). */
export function labBridges(env = labEnv(), dead = false) {
  const line = (url) => `snowflake 192.0.2.3:80 ${env.BRIDGE_FP} fingerprint=${env.BRIDGE_FP} url=${url} ice=${env.STUN_URL}`;
  return dead ? line(`${DEAD_BROKER}/`) : `${line(`${DEAD_BROKER}/`)}\n${line(env.BROKER_URL)}`;
}

const lab = LIVE ? null : labConfig();

/** Static server of the repository, with the pages' CSP also naming the lab broker (lab only). */
export function serveTor(route = null) {
  if (LIVE) return serve(null, route);
  const origins = `${DEAD_BROKER} ${new URL(lab.labBroker).origin}`;
  return serve((p, read) => (/\/(tor|index)\.html$/.test(p) ? read().replace("connect-src 'self' https:", `connect-src 'self' https: ${origins}`) : null), route);
}

/** Prepares a browser context for tor.html (the lab settings, unless LIVE). */
export async function torContext(ctx, extra = {}) {
  if (lab) await ctx.addInitScript((c) => { globalThis.ephemTorLab = c; }, { ...lab, ...extra });
  else if (process.env.TOR_LOG) await ctx.addInitScript((l) => { globalThis.ephemTorLog = l; }, process.env.TOR_LOG);
  return ctx;
}

/** Waits until `page`'s onion service is up, printing the Tor status every 10 s meanwhile. */
export async function torReady(page, who) {
  const t0 = Date.now();
  let last = '';
  const tick = setInterval(async () => {
    const s = await page.textContent('#tor-state').catch(() => '');
    if (s && s !== last) console.log(`  ${who} after ${Math.round((Date.now() - t0) / 1000)} s: ${s}`);
    last = s;
  }, 10_000);
  try {
    await page.waitForFunction(() => /Reachable through Tor|Tor failed/.test(document.querySelector('#tor-state')?.textContent), null, { timeout: T });
    const s = await page.textContent('#tor-state');
    if (/Tor failed/.test(s)) throw new Error(`${who}: ${s}`);
  } finally {
    clearInterval(tick);
  }
}

/** Browser noise that says nothing about the app. */
export const noise = (text) => /Password field is not contained in a form/.test(text);

/** Page problems, without the browser's own report of the dead broker (lab only, expected). */
export const unexpected = (problems) => problems.filter((p) => LIVE || !/net::ERR_CONNECTION_REFUSED/.test(p));

/** Console lines of every page (`record(page, who)`), printed by `dumpLogs()` when a flow fails. */
const logs = [];
export function record(page, who) {
  page.on('console', (m) => {
    if (!/ERR_CONNECTION_REFUSED/.test(m.text())) logs.push(`  ${who} | ${m.text()}`);
    if (process.env.VERBOSE && !noise(m.text())) console.log(`  ${who} |`, m.text());
  });
}
/** The app's own Tor lines (`tor: …`) in full, then the last `n` other lines. */
export const dumpLogs = (n = 40) => {
  if (process.env.VERBOSE) return;
  const quiet = (l) => /unreachable|proxy ready|memquota|Downloading|Marked consensus|Directory is complete|consensus diff/.test(l);
  console.log(logs.filter((l) => /\btor: /.test(l)).join('\n'));
  console.log('  ---');
  console.log(logs.filter((l) => !quiet(l)).slice(-n).join('\n'));
};
