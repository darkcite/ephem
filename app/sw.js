// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Ephem service worker (docs/P2P-CHAT.md §17.1).
// - Caches only the static app files below; never sees codes (URL fragments are not sent) or messages.
// - Pins the version: a new sw.js installs and WAITS; the page shows the new build and asks the
//   user before it is activated (message 'activate').
// - Tor mode (tor.html and the Tor build, §28.2) is cached on first use only: direct users never
//   download it. It belongs to the same version (the build id covers it).
// VERSION, FILES and TOR_FILES are written by tools/stamp.py (run by ./build.sh).
const VERSION = '33b1ece3d0fa';
const FILES = ["./", "index.html", "app.css", "app.js", "slots.js", "bridges.js", "channels.js", "ui.js", "pkg/ephem.js", "pkg/ephem_bg.wasm", "manifest.webmanifest", "icons/icon.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/icon-maskable-512.png", "icons/apple-touch-icon.png"];
const TOR_FILES = ["tor.html", "pkg/ephem_tor.js", "pkg/ephem_tor_bg.wasm", "channel.html", "redirect.js"];
const CACHE = `ephem-${VERSION}`;
const TOR_PATHS = new Set(TOR_FILES.map((f) => new URL(f, self.location).pathname));

self.addEventListener('install', (e) => {
  // cache: 'reload' bypasses the HTTP cache so the precache matches this version exactly.
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES.map((f) => new Request(f, { cache: 'reload' })))));
});

self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    for (const k of await caches.keys()) if (k !== CACHE) await caches.delete(k);
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET' || new URL(req.url).origin !== self.location.origin) return;
  e.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const hit = await cache.match(req, { ignoreSearch: true });
    if (hit) return hit;
    const res = await fetch(req);
    if (res.ok && TOR_PATHS.has(new URL(req.url).pathname)) await cache.put(req, res.clone());
    return res;
  })());
});

self.addEventListener('message', (e) => {
  if (e.data === 'version') e.source.postMessage({ version: VERSION });
  else if (e.data === 'activate') self.skipWaiting();
});
