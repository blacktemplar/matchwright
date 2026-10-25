// What makes the page an app, run before its WebAssembly: registers the service worker `sw.js`,
// which keeps the page working offline, and keeps the browser's offer to install the page, which may
// come before the WebAssembly runs, for the page to make (`src/install.rs`).
"use strict";

if ("serviceWorker" in navigator) {
  // On the first visit the page loads before the worker takes it over, so the worker keeps what
  // the page loads only from then on. It is handed what the page loaded until then, including
  // what was still loading.
  if (!navigator.serviceWorker.controller) {
    const loaded = [location.href.split("#")[0]];
    let worker = null;
    new PerformanceObserver((entries) => {
      const urls = entries.getEntries().map((entry) => entry.name);
      if (worker) {
        worker.postMessage(urls);
      } else {
        loaded.push(...urls);
      }
    }).observe({ type: "resource", buffered: true });
    navigator.serviceWorker.ready.then((registration) => {
      worker = registration.active;
      worker.postMessage(loaded);
    });
  }
  navigator.serviceWorker.register("sw.js").catch((error) => {
    console.warn("the page cannot be kept for offline use:", error);
  });
}

// the offer, as `tournamentInstallPrompt`, announced by the event `tournament-installable`
window.addEventListener("beforeinstallprompt", (event) => {
  event.preventDefault();
  window.tournamentInstallPrompt = event;
  window.dispatchEvent(new Event("tournament-installable"));
});
