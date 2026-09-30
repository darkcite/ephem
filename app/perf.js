// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Debug readout (Settings → About → Performance; optional overlay): latency, CPU load and memory
// as far as the browser lets a page see them. Nothing leaves the device.
//
// - Latency: per chat, send → "delivered" receipt (a full round trip through the real path,
//   Tor or direct, including the peer's app); direct chats also report WebRTC's own RTT.
// - CPU: no browser API. The main thread is sampled with a 100 ms timer: how late it fires is
//   time the thread was busy (our code, the browser, or the OS throttling a background tab).
//   Chromium also reports "long tasks" (≥ 50 ms).
// - Memory: the WebAssembly memories (the app's and, in direct mode, the Tor part's), plus the JS
//   heap where Chromium exposes it (`performance.memory`; Safari has no such figure).
// Setup/UI path: small rings, sampled a few times a second.

const TICK_MS = 100;
const WINDOW = 600;                  // samples kept: the last minute
const LAT_KEEP = 20;                 // delivery times kept per chat

const lags = new Float32Array(WINDOW);
let lagN = 0;
let lagAt = 0;
let last = performance.now();
let longTasks = 0;
let longMs = 0;
const started = performance.now();
let sources = { memories: () => [], chats: () => [] };

setInterval(() => {
  const now = performance.now();
  lags[lagAt] = Math.max(0, now - last - TICK_MS);
  lagAt = (lagAt + 1) % WINDOW;
  lagN = Math.min(lagN + 1, WINDOW);
  last = now;
}, TICK_MS);

try {
  new PerformanceObserver((l) => {
    for (const e of l.getEntries()) {
      longTasks++;
      longMs += e.duration;
    }
  }).observe({ type: 'longtask', buffered: true });
} catch { /* not in Safari or Firefox */ }

/** Where the numbers come from: `memories()` → [[label, bytes]], `chats()` → [{ name, via, lat: [ms…], rtt }]. */
export function init(s) {
  sources = s;
}

/** A delivery time (ms) for a chat's ring (`c.lat`). */
export function delivered(c, ms) {
  if (!c.lat) c.lat = [];
  c.lat.push(ms);
  if (c.lat.length > LAT_KEEP) c.lat.shift();
}

const median = (a) => { if (!a.length) return NaN; const s = [...a].sort((x, y) => x - y); return s[s.length >> 1]; };
const mb = (b) => `${(b / 1048576).toFixed(1)} MB`;
const ms = (x) => (Number.isFinite(x) ? `${Math.round(x)} ms` : '-');

/** Main-thread load over the last minute: busy share, median and worst timer lateness. */
function load() {
  const a = Array.from(lags.subarray(0, lagN));
  const busy = a.reduce((s, x) => s + x, 0) / Math.max(1, lagN * TICK_MS + a.reduce((s, x) => s + x, 0));
  return { busy, p50: median(a), max: a.length ? Math.max(...a) : NaN };
}

/** The full readout (Settings → About → Performance). */
export function report() {
  const l = load();
  const lines = [
    `uptime           ${Math.round((performance.now() - started) / 1000)} s`,
    `main thread      ~${(l.busy * 100).toFixed(1)} % busy (last minute); timer late p50 ${ms(l.p50)}, max ${ms(l.max)}`,
    `long tasks       ${'PerformanceObserver' in globalThis && PerformanceObserver.supportedEntryTypes?.includes('longtask') ? `${longTasks} (${Math.round(longMs)} ms in all)` : 'not reported by this browser'}`,
  ];
  for (const [label, bytes] of sources.memories()) lines.push(`${label.padEnd(17)}${mb(bytes)}`);
  const pm = performance.memory;
  lines.push(`JS heap          ${pm ? `${mb(pm.usedJSHeapSize)} of ${mb(pm.jsHeapSizeLimit)}` : 'not reported by this browser'}`);
  lines.push(`cores            ${navigator.hardwareConcurrency || '?'}`);
  const chats = sources.chats();
  if (!chats.length) lines.push('latency          no open chat');
  for (const c of chats) {
    lines.push(`${`latency ${c.name}`.slice(0, 16).padEnd(17)}${c.via}: delivered in ${ms(c.lat.at(-1))} (median ${ms(median(c.lat))} of ${c.lat.length})${c.rtt ? `, RTT ${c.rtt}` : ''}`);
  }
  return lines.join('\n');
}

/** One line for the overlay. */
export function brief() {
  const l = load();
  const wasmBytes = sources.memories().reduce((s, [, b]) => s + b, 0);
  const lat = sources.chats().map((c) => `${c.name.slice(0, 8)} ${ms(c.lat.at(-1))}`).join(' · ');
  return `CPU ~${(l.busy * 100).toFixed(0)}% · lag ${ms(l.max)} · wasm ${mb(wasmBytes)}${performance.memory ? ` · JS ${mb(performance.memory.usedJSHeapSize)}` : ''}${lat ? ` · ${lat}` : ''}`;
}
