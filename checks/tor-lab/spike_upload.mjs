// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spike B-P5 (docs/BOARDS.md G.16): how long does a board post take to upload from one tab to
// an onion hosted by another tab, each with its own arti over Snowflake? Sizes: a text post
// (4 KiB), a small and a large image post (128 KiB, 512 KiB). Each upload opens a new stream
// (as a poster's submit does); `open` is the stream set-up, `send` the transfer and the host's
// answer. Needs `checks/tor-lab/build_web.sh` and the lab (`lab.sh up`), or RELAY=1 with
// `lab.sh relay` for the real Tor network.
//
//   node checks/tor-lab/spike_upload.mjs          REPS=5 by default
import { launch, serve, watch } from '../e2e_lib.mjs';
import { RELAY, transportArgs } from './tor_env.mjs';

const SIZES = [4 << 10, 128 << 10, 512 << 10];
const REPS = Number(process.env.REPS || 5);
const srv = await serve();
const browser = await launch();
const url = `http://127.0.0.1:${srv.address().port}/checks/tor-lab/web/index.html`;
const args = transportArgs();
const q = (a, p) => a.slice().sort((x, y) => x - y)[Math.min(a.length - 1, Math.floor(p * a.length))];
try {
  const [host, poster] = [await (await browser.newContext()).newPage(), await (await browser.newContext()).newPage()];
  for (const [p, w] of [[host, 'host'], [poster, 'poster']]) {
    watch(p, w);
    await p.goto(url);
    await p.waitForFunction(() => typeof window.hostSink === 'function');
  }
  const t0 = Date.now();
  const { onion } = await host.evaluate((a) => window.hostSink(a), args);
  console.log(`  ${RELAY ? 'real Tor network (relay)' : 'lab'}: host up with ${onion} in ${Date.now() - t0} ms`);
  const runs = await poster.evaluate(([a, o, s, r]) => window.uploads(a, o, s, r), [args, onion, SIZES, REPS]);
  console.log('  size     | ok  | open ms (median / p95) | send ms (median / p95) | KiB/s (median)');
  for (const size of SIZES) {
    const r = runs.filter((x) => x.size === size);
    const ok = r.filter((x) => x.ok);
    const open = ok.map((x) => x.open), send = ok.map((x) => x.send);
    const rate = ok.map((x) => (size / 1024) / (x.send / 1000));
    console.log(`  ${String(size >> 10).padStart(4)} KiB | ${ok.length}/${r.length} | ${String(q(open, 0.5)).padStart(6)} / ${String(q(open, 0.95)).padEnd(6)}        | ${String(q(send, 0.5)).padStart(6)} / ${String(q(send, 0.95)).padEnd(6)}        | ${Math.round(q(rate, 0.5))}`);
    for (const e of r.filter((x) => x.error)) console.log(`      error: ${e.error}`);
  }
} catch (e) {
  console.log('  spike failed:', e.message.split('\n')[0]);
} finally {
  await browser.close();
  srv.close();
}
