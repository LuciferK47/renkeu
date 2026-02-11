// Minimal service worker for static asset caching
const CACHE_NAME = 'latex-ai-static-v1'
const ASSET_MATCH = /\/assets\//

self.addEventListener('install', (event) => {
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.map((k) => (k !== CACHE_NAME ? caches.delete(k) : Promise.resolve()))))
  )
  self.clients.claim()
})

self.addEventListener('fetch', (event) => {
  const { request } = event
  const url = new URL(request.url)
  // Cache hashed assets aggressively
  if (ASSET_MATCH.test(url.pathname) && request.method === 'GET') {
    event.respondWith(
      caches.open(CACHE_NAME).then(async (cache) => {
        const cached = await cache.match(request)
        if (cached) return cached
        const response = await fetch(request)
        // Only cache successful opaque or basic responses
        if (response && (response.status === 200 || response.type === 'opaque')) {
          cache.put(request, response.clone())
        }
        return response
      })
    )
  }
})
