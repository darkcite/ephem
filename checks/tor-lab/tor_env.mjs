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

function labConfig() {
  const env = Object.fromEntries(fs.readFileSync('/tmp/ephlab/lab.env', 'utf8').trim().split('\n').map((l) => l.split('=')));
  return {
    broker: env.BROKER_URL,
    fingerprint: env.BRIDGE_FP,
    ice: env.STUN_URL,
    nat: 'unrestricted',
    network: fs.readFileSync(`${env.LAB}/arti-net.toml`, 'utf8'),
    log: process.env.TOR_LOG || 'warn',
  };
}

const lab = LIVE ? null : labConfig();

/** Static server of the repository, with tor.html's CSP pointed at the lab broker (lab only). */
export function serveTor() {
  if (LIVE) return serve();
  const origin = new URL(lab.broker).origin;
  return serve((p, read) => (p.endsWith('/tor.html') ? read().replace('https://snowflake-broker.torproject.net', origin) : null));
}

/** Prepares a browser context for tor.html (the lab settings, unless LIVE). */
export async function torContext(ctx) {
  if (lab) await ctx.addInitScript((c) => { globalThis.ephemTorLab = c; }, lab);
  return ctx;
}
