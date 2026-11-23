const CACHE_NAME = 'watchlist-shell-v1';
const SHELL_ASSETS = [
  '/static/style.css',
  '/static/htmx.min.js',
  '/static/manifest.json',
  '/static/icon-192.png',
  '/static/icon-512.png'
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL_ASSETS))
  );
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
    )
  );
  self.clients.claim();
});

self.addEventListener('fetch', (event) => {
  if (event.request.method !== 'GET') return;
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;

  if (SHELL_ASSETS.includes(url.pathname)) {
    event.respondWith(
      caches.match(event.request).then((cached) => cached || fetch(event.request))
    );
    return;
  }

  // Network-first for HTML pages; show clean offline notice if server is unreachable
  if (event.request.mode === 'navigate') {
    event.respondWith(
      fetch(event.request).catch(
        () =>
          new Response(
            '<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><body style="background:#0b0d11;color:#e9edf5;font-family:system-ui;padding:24px;text-align:center"><h2>Watchlist Offline</h2><p>Cannot reach your Watchlist server right now. Check your connection and try again.</p></body>',
            { headers: { 'Content-Type': 'text/html; charset=utf-8' } }
          )
      )
    );
  }
});
