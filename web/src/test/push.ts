import { vi } from 'vitest'

export const ENDPOINT = 'https://push.example/send/abc'
export const STORE_KEY = 'postit.push.endpoints'
/** The signed-in user of `meFixture`. */
export const USER = '00000000-0000-7000-8000-000000000001'

/** The stored push map with `endpoint` registered by `user` under the server ID `id`. */
export function stored(id: string, user = USER, endpoint = ENDPOINT): string {
  return JSON.stringify({ [endpoint]: { id, user } })
}

export interface FakeSubscription {
  endpoint: string
  toJSON: () => { endpoint: string; keys: { p256dh: string; auth: string } }
  unsubscribe: ReturnType<typeof vi.fn<() => Promise<boolean>>>
}

export function fakeSubscription(): FakeSubscription {
  return {
    endpoint: ENDPOINT,
    toJSON: () => ({ endpoint: ENDPOINT, keys: { p256dh: 'BP256dhKey', auth: 'AuthSecret' } }),
    unsubscribe: vi.fn(() => Promise.resolve(true)),
  }
}

export interface PushEnv {
  notification: { permission: NotificationPermission; requestPermission: ReturnType<typeof vi.fn> }
  subscribe: ReturnType<typeof vi.fn>
  getSubscription: ReturnType<typeof vi.fn>
  subscription: FakeSubscription
  restore: () => void
}

/**
 * Gives jsdom a browser with Web Push: `Notification`, `PushManager`, and a service worker whose
 * registration holds the subscription when `existing` is set. `ask` is what the permission prompt answers.
 */
export function stubPush({
  permission = 'default',
  ask = 'granted',
  existing = false,
  userAgent,
}: {
  permission?: NotificationPermission
  ask?: NotificationPermission
  existing?: boolean
  userAgent?: string
} = {}): PushEnv {
  const subscription = fakeSubscription()
  const notification = { permission, requestPermission: vi.fn(() => Promise.resolve(ask)) }
  const getSubscription = vi.fn(() => Promise.resolve(existing ? subscription : null))
  const subscribe = vi.fn(() => Promise.resolve(subscription))
  const registration = { pushManager: { getSubscription, subscribe } }
  vi.stubGlobal('Notification', notification)
  vi.stubGlobal('PushManager', vi.fn())
  Object.defineProperty(navigator, 'serviceWorker', {
    configurable: true,
    value: {
      ready: Promise.resolve(registration),
      register: vi.fn(() => Promise.resolve(registration)),
      getRegistration: vi.fn(() => Promise.resolve(registration)),
    },
  })
  if (userAgent) Object.defineProperty(navigator, 'userAgent', { configurable: true, value: userAgent })
  return {
    notification,
    subscribe,
    getSubscription,
    subscription,
    restore: () => {
      vi.unstubAllGlobals()
      Reflect.deleteProperty(navigator, 'serviceWorker')
      Reflect.deleteProperty(navigator, 'userAgent')
      localStorage.clear()
    },
  }
}
