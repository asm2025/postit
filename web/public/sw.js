// postit service worker: shows Web Push notifications and routes clicks into the app.
// Plain JavaScript at the site root (scope "/"); Vite copies public/ unchanged and the server
// serves it with no-cache so a new version is picked up on the next visit.
// The push payload is { id, kind, title, body, tag } with a localized title and body.

self.addEventListener('push', (event) => {
  let message = {}
  try {
    message = event.data ? event.data.json() : {}
  } catch {
    // A payload that is not JSON still gets a visible notice: browsers require one per push.
  }
  const title = typeof message.title === 'string' && message.title ? message.title : 'postit'
  const options = {
    body: typeof message.body === 'string' ? message.body : '',
    data: { id: typeof message.id === 'string' ? message.id : null },
  }
  // One job's notices share a tag, so each replaces the last instead of piling up. `renotify`
  // without a tag makes showNotification throw a TypeError, so it is set only with a tag.
  if (typeof message.tag === 'string' && message.tag) {
    options.tag = message.tag
    options.renotify = true
  }
  event.waitUntil(self.registration.showNotification(title, options))
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const id = event.notification.data && event.notification.data.id
  const url = id ? `/?notification=${encodeURIComponent(id)}` : '/'
  event.waitUntil(
    (async () => {
      const open = await self.clients.matchAll({ type: 'window', includeUncontrolled: true })
      const client = open[0]
      if (client) {
        await client.focus()
        if (typeof client.navigate === 'function') await client.navigate(url)
        return
      }
      await self.clients.openWindow(url)
    })(),
  )
})
