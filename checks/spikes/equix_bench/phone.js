// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P1b (docs/BOARDS.md G.16.2): Equi-X timing on the owner's phones and browsers, one core
// then several Workers, as the boards' poster will solve (crates/board/src/pow.rs). Effort E costs
// E solutions on average; from B-P1, the median solve time is ≈ 0.69 × the mean, p95 ≈ 3 × it.

const $ = (id) => document.getElementById(id);
const out = (t) => { $('out').textContent = t; };

/** `w` module workers solving for about `ms`; total solutions and wall time. */
function workers(w, ms) {
  return new Promise((resolve, reject) => {
    const res = [];
    const t0 = performance.now();
    for (let j = 0; j < w; j++) {
      const wk = new Worker('./bench.js', { type: 'module' });
      wk.onerror = (e) => reject(new Error(e.message || 'worker failed'));
      wk.onmessage = (e) => {
        res.push(e.data);
        wk.terminate();
        if (res.length === w) resolve({ sols: res.reduce((s, r) => s + r.sols, 0), attempts: res.reduce((s, r) => s + r.solve.length, 0), wall: performance.now() - t0 });
      };
      // ~90 ms per attempt on a desktop core: size the run to the time budget.
      wk.postMessage({ k: Math.max(20, Math.round(ms / 90)), base: 7_000_000 + j * 100_000 });
    }
  });
}

const sec = (x) => (x < 100 ? `${x.toFixed(1)} s` : `${Math.round(x / 60)} min`);

async function run() {
  $('run').disabled = true;
  $('copy').hidden = true;
  let lock = null;
  try { lock = await navigator.wakeLock?.request('screen'); } catch { /* not available */ }
  const cores = navigator.hardwareConcurrency || 2;
  const many = Math.min(4, Math.max(2, cores));
  try {
    out('Warming up…');
    await workers(1, 2_000);
    out('One core (15 s)…');
    const one = await workers(1, 15_000);
    out(`One core: ${(one.sols / one.wall * 1000).toFixed(1)} solutions/s. Now ${many} workers (20 s)…`);
    const all = await workers(many, 20_000);
    const r1 = one.sols / one.wall * 1000;
    const rn = all.sols / all.wall * 1000;
    const median = (e, r) => sec(0.69 * e / r);
    const p95 = (e, r) => sec(3 * e / r);
    const e10 = Math.round(10 * rn / 0.69);
    out([
      `Ephem B-P1b result (${new Date().toISOString().slice(0, 16)}Z)`,
      `device: ${navigator.userAgent}`,
      `cores reported: ${cores}; workers used: ${many}`,
      `one core:   ${r1.toFixed(1)} solutions/s (${one.attempts} attempts, ${(one.attempts / one.wall * 1000).toFixed(1)} attempts/s)`,
      `${many} workers: ${rn.toFixed(1)} solutions/s (${(rn / r1).toFixed(2)}x)`,
      '',
      `reply at effort 350: median ${median(350, rn)}, p95 ${p95(350, rn)}`,
      `new thread at effort 2800:          median ${median(2800, rn)}, p95 ${p95(2800, rn)}`,
      `effort for a 10 s median reply here: ${e10} (thread x8: ${e10 * 8})`,
    ].join('\n'));
    $('copy').hidden = false;
  } catch (e) {
    out(`Failed: ${e.message || e}`);
  } finally {
    lock?.release?.();
    $('run').disabled = false;
  }
}

$('run').onclick = run;
$('copy').onclick = () => navigator.clipboard.writeText($('out').textContent).then(() => { $('copy').textContent = 'Copied'; });
