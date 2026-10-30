/// <reference types="node" />
import { readFileSync } from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { vi } from 'vitest'

type Handler = (event: unknown) => void

// public/sw.js is plain JavaScript run by the browser, not bundled: load it as text into a fake
// worker scope, the way the browser would run it.
function loadWorker() {
  const source = readFileSync(path.join(import.meta.dirname, '../../../public/sw.js'), 'utf8')
  const handlers = new Map<string, Handler>()
  const scope = {
    addEventListener: (type: string, handler: Handler) => handlers.set(type, handler),
    registration: { showNotification: vi.fn(() => Promise.resolve()) },
    clients: {
      matchAll: vi.fn((): Promise<unknown[]> => Promise.resolve([])),
      openWindow: vi.fn(() => Promise.resolve(null)),
    },
    skipWaiting: vi.fn(() => Promise.resolve()),
  }
  vm.runInNewContext(source, { self: scope })
  return { scope, handlers }
}

/** An event whose `waitUntil` promise the test can await. */
function event<T extends object>(extra: T) {
  let pending: Promise<unknown> = Promise.resolve()
  return {
    event: {
      ...extra,
      waitUntil: (p: Promise<unknown>) => {
        pending = p
      },
    },
    settled: () => pending,
  }
}

const ID = '018f0000-0000-7000-8000-000000000001'
const TAG = '018f0000-0000-7000-8000-0000000000aa'

it('push_event_shows_notification', async () => {
  const { scope, handlers } = loadWorker()
  const e = event({
    data: {
      json: () => ({ id: ID, kind: 'delivery_failed', title: 'Post failed', body: 'Mastodon needs a look', tag: TAG }),
    },
  })
  handlers.get('push')?.(e.event)
  await e.settled()
  expect(scope.registration.showNotification).toHaveBeenCalledWith('Post failed', {
    body: 'Mastodon needs a look',
    tag: TAG,
    renotify: true,
    data: { id: ID },
  })
})

it('push without readable data still shows a generic notification instead of nothing', async () => {
  const { scope, handlers } = loadWorker()
  const e = event({
    data: {
      json: () => {
        throw new Error('not json')
      },
    },
  })
  handlers.get('push')?.(e.event)
  await e.settled()
  expect(scope.registration.showNotification).toHaveBeenCalledTimes(1)
})

describe('notificationclick', () => {
  function click() {
    const close = vi.fn()
    return { close, ...event({ notification: { close, data: { id: ID } } }) }
  }

  it('click_focuses_and_navigates', async () => {
    const { scope, handlers } = loadWorker()
    const client = { focus: vi.fn(() => Promise.resolve()), navigate: vi.fn(() => Promise.resolve()) }
    scope.clients.matchAll.mockResolvedValue([client])
    const c = click()
    handlers.get('notificationclick')?.(c.event)
    await c.settled()
    expect(c.close).toHaveBeenCalled()
    expect(client.focus).toHaveBeenCalled()
    expect(client.navigate).toHaveBeenCalledWith(`/?notification=${ID}`)
    expect(scope.clients.openWindow).not.toHaveBeenCalled()
  })

  it('opens a window when no postit client is open', async () => {
    const { scope, handlers } = loadWorker()
    const c = click()
    handlers.get('notificationclick')?.(c.event)
    await c.settled()
    expect(scope.clients.openWindow).toHaveBeenCalledWith(`/?notification=${ID}`)
  })
})
