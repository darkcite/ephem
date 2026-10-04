// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P11 (docs/BOARDS.md G.16.2): the reachability of a board hosted from a tab, over hours,
// as a reader on another network sees it. Uses the app's Tor build (BoardApp.read) as is.
import init, { ChannelApp, BoardApp } from '../../../app/pkg/ephem_tor.js';
import { DEFAULT_BRIDGES } from '../../../app/bridges.js';

const EVERY_MS = 5 * 60 * 1000;
const $ = (id) => document.getElementById(id);
const rows = [];
let started = 0;
let lock = null;

const pct = (a, b) => (b ? `${((100 * a) / b).toFixed(1)} %` : '–');
const hm = (ms) => `${Math.floor(ms / 3_600_000)} h ${Math.round((ms % 3_600_000) / 60_000)} min`;

function render(status) {
  const ok = rows.filter((r) => r.ok).length;
  let worst = 0;
  let run = 0;
  for (const r of rows) {
    run = r.ok ? 0 : run + 1;
    worst = Math.max(worst, run);
  }
  const times = rows.filter((r) => r.ok).map((r) => r.ms).sort((a, b) => a - b);
  const med = times.length ? times[Math.floor(times.length / 2)] : 0;
  $('out').textContent = [
    `Ephem B-P11 result (${new Date().toISOString().slice(0, 16)}Z)`,
    `probe: ${navigator.userAgent}`,
    `running for ${hm(started ? Date.now() - started : 0)}; ${rows.length} reads, every 5 min`,
    `reachable: ${ok}/${rows.length} = ${pct(ok, rows.length)}; longest outage: ${worst * 5} min; median read ${(med / 1000).toFixed(1)} s`,
    '',
    status,
    '',
    ...rows.slice(-48).reverse().map((r) => `${r.at.slice(11, 16)}Z ${r.ok ? `ok ${(r.ms / 1000).toFixed(1)} s, version ${r.seq}` : `FAIL ${r.err.slice(0, 80)}`}`),
  ].join('\n');
  $('copy').hidden = !rows.length;
}

async function start() {
  const p = new URLSearchParams(($('link').value.split('#')[1] || ''));
  const name = p.get('B');
  const onions = [p.get('o'), ...(p.get('m') || '').split(',')].filter(Boolean).join(',');
  if (!name || !onions) return render('That is not a board link (#B=…&o=…).');
  $('start').disabled = true;
  try { lock = await navigator.wakeLock?.request('screen'); } catch { /* not available */ }
  render('Starting Tor (Snowflake)…');
  await init({ module_or_path: new URL('../../../app/pkg/ephem_tor_bg.wasm', import.meta.url) });
  const ch = new ChannelApp();
  const log = new URLSearchParams(location.search).get('log'); // ?log=debug: arti's log in the console
  if (log) ch.tor_log(log);
  const boards = new BoardApp(ch);
  const lab = globalThis.ephemTorLab; // the offline lab's network, when its checks drive this page
  await ch.tor_start(lab?.bridges || DEFAULT_BRIDGES, lab?.nat || '', lab?.network || '', new Uint8Array());
  started = Date.now();
  for (;;) {
    const t0 = performance.now();
    const at = new Date().toISOString();
    try {
      const v = JSON.parse(await boards.read(name, onions, 0, [], false));
      rows.push({ at, ok: true, ms: performance.now() - t0, seq: v.sequence });
    } catch (e) {
      rows.push({ at, ok: false, err: String(e?.message || e) });
    }
    render(`Tor: ${ch.tor_status()}. Next read in 5 minutes. Keep this page open.`);
    await new Promise((r) => setTimeout(r, EVERY_MS));
  }
}

$('start').onclick = start;
$('copy').onclick = () => navigator.clipboard.writeText($('out').textContent).then(() => { $('copy').textContent = 'Copied'; });
document.addEventListener('visibilitychange', async () => {
  if (document.visibilityState === 'visible' && started && !lock) {
    try { lock = await navigator.wakeLock?.request('screen'); } catch { /* not available */ }
  }
});
