// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem service worker (docs/P2P-CHAT.md §17.1).
// - Caches only the static app files below; never sees codes (URL fragments are not sent) or messages.
// - Pins the version the user accepted: a new sw.js precaches its files into its own cache, but
//   every request is answered from the ACCEPTED cache until the user taps Update (message
//   'activate'). Closing and reopening the app no longer switches versions (security audit W-1).
//   What a service worker cannot stop: the host serving a hostile sw.js that does something else
//   (§5, §17.1, §21 say so).
// - The Tor build's pages (tor.html, channel.html) are precached with the app (W-3); only the
//   Tor build itself (pkg/ephem_tor*, §28.2, integrity-pinned by the pages) is cached on first
//   use, so direct users never download it. The build id covers it.
// VERSION, FILES and TOR_FILES are written by tools/stamp.py (run by ./build.sh).
const VERSION = '9790c540b3cd';
const FILES = ["./", "index.html", "tor.html", "channel.html", "redirect.js", "app.css", "app.js", "slots.js", "bridges.js", "channels.js", "boards.js", "pow-worker.js", "store-worker.js", "ui.js", "perf.js", "pkg/ephem.js", "pkg/ephem_bg.wasm", "manifest.webmanifest", "icons/icon.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/icon-maskable-512.png", "icons/apple-touch-icon.png"];
const TOR_FILES = ["pkg/ephem_tor.js", "pkg/ephem_tor_bg.wasm", "pkg/ephem_pow.wasm"];
const CACHE = `ephem-${VERSION}`;
const TOR_PATHS = new Set(TOR_FILES.map((f) => new URL(f, self.location).pathname));
// The accepted version: one entry in its own cache (survives worker updates, unlike globals).
const META = 'ephem-meta';
const ACCEPTED = new URL('__accepted', self.registration.scope).href;

async function accepted() {
  const r = await (await caches.open(META)).match(ACCEPTED);
  return r ? r.text() : null;
}

async function accept(name) {
  await (await caches.open(META)).put(ACCEPTED, new Response(name));
  for (const k of await caches.keys()) if (k !== name && k !== META) await caches.delete(k);
}

self.addEventListener('install', (e) => {
  // cache: 'reload' bypasses the HTTP cache so the precache matches this version exactly.
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES.map((f) => new Request(f, { cache: 'reload' })))));
});

self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    const was = await accepted();
    // First install (or a worker from before this scheme): nothing else was ever accepted.
    if (!was || !(await caches.has(was))) await accept(CACHE);
    else for (const k of await caches.keys()) if (k !== was && k !== CACHE && k !== META) await caches.delete(k);
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET' || new URL(req.url).origin !== self.location.origin) return;
  e.respondWith((async () => {
    const cache = await caches.open((await accepted()) || CACHE);
    const hit = await cache.match(req, { ignoreSearch: true });
    if (hit) return hit;
    const res = await fetch(req);
    // Only the lazily cached Tor build, never a page (W-3).
    if (res.ok && TOR_PATHS.has(new URL(req.url).pathname) && !(res.headers.get('content-type') || '').includes('text/html')) await cache.put(req, res.clone());
    return res;
  })());
});

self.addEventListener('message', (e) => {
  e.waitUntil((async () => {
    if (e.data === 'version') {
      const a = await accepted();
      e.source.postMessage({ version: VERSION, accepted: a ? a.slice('ephem-'.length) : VERSION });
    } else if (e.data === 'activate') {
      await accept(CACHE);
      await self.skipWaiting();
      e.source.postMessage({ accepted: VERSION });
    }
  })());
});
