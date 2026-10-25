// The service worker that keeps the page working offline: everything the page loads is kept in a
// cache. The files under `assets/` are named after their content, so once kept they come from the
// cache. Everything else (the page itself, its manifest and icons) comes from the network and is kept
// as it comes, and comes from the cache only when the network fails. A page that differs from the
// one kept is another version, and the assets kept are dropped before it loads its own.
"use strict";

const CACHE = "tournament-gui";
const ASSETS = new URL("assets/", self.registration.scope).href;

self.addEventListener("install", () => self.skipWaiting());

// takes over the pages open, so that a page opened before is kept from now on
self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE) {
          await caches.delete(name);
        }
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) {
    return;
  }
  event.respondWith(request.url.startsWith(ASSETS) ? fromCache(request) : fromNetwork(request));
});

// the URLs of what a page loaded before this worker took it over, see `pwa.js`
self.addEventListener("message", (event) => {
  if (Array.isArray(event.data)) {
    event.waitUntil(keep(event.data));
  }
});

async function fromCache(request) {
  const cache = await caches.open(CACHE);
  const kept = await cache.match(request);
  if (kept) {
    return kept;
  }
  const response = await fetch(request);
  if (response.ok) {
    await cache.put(request, response.clone());
  }
  return response;
}

async function fromNetwork(request) {
  const cache = await caches.open(CACHE);
  let response;
  try {
    response = await fetch(request);
  } catch (error) {
    const kept = await cache.match(request, { ignoreSearch: true });
    if (kept) {
      return kept;
    }
    throw error;
  }
  if (!response.ok) {
    // e.g. a proxy's answer that the server cannot be reached
    return (await cache.match(request, { ignoreSearch: true })) ?? response;
  }
  if (request.mode === "navigate") {
    const kept = await cache.match(request, { ignoreSearch: true });
    if (kept && (await kept.text()) !== (await response.clone().text())) {
      // before the response, so that none of the new version's assets is dropped
      for (const key of await cache.keys()) {
        if (key.url.startsWith(ASSETS)) {
          await cache.delete(key);
        }
      }
    }
  }
  await cache.put(request, response.clone());
  return response;
}

async function keep(urls) {
  const cache = await caches.open(CACHE);
  for (const url of urls) {
    if (new URL(url).origin !== self.location.origin || (await cache.match(url))) {
      continue;
    }
    try {
      await cache.add(url);
    } catch {
      // kept once loaded again
    }
  }
}
