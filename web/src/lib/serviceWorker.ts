/**
 * Registers the Web Push service worker when the browser has one. It is optional: a browser without
 * service workers, or a failed registration, changes nothing else, so errors are swallowed.
 */
export function registerServiceWorker(): void {
  if (!('serviceWorker' in navigator)) return
  try {
    navigator.serviceWorker.register('/sw.js').catch(() => undefined)
  } catch {
    /* register threw synchronously (insecure context): same as unsupported */
  }
}
