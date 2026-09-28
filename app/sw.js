// Ephem service worker (docs/P2P-CHAT.md §17.1).
// - Caches only the static app files below; never sees codes (URL fragments are not sent) or messages.
// - Pins the version: a new sw.js installs and WAITS; the page shows the new build and asks the
//   user before it is activated (message 'activate').
// VERSION and FILES are written by tools/stamp.py (run by ./build.sh).
const VERSION = 'c9bdc289a25d';
const FILES = ["./", "index.html", "app.css", "app.js", "pkg/ephem.js", "pkg/ephem_bg.wasm", "manifest.webmanifest", "icons/icon.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/icon-maskable-512.png", "icons/apple-touch-icon.png"];
const CACHE = `ephem-${VERSION}`;

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
  e.respondWith((async () => (await (await caches.open(CACHE)).match(req, { ignoreSearch: true })) || fetch(req))());
});

self.addEventListener('message', (e) => {
  if (e.data === 'version') e.source.postMessage({ version: VERSION });
  else if (e.data === 'activate') self.skipWaiting();
});
