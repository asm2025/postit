import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useState } from 'react'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap, type ApiClient } from '@/api/client'
import { ApiError } from '@/api/errors'

/**
 * `unsupported`: no Web Push in this browser, or the server has push off. `blocked`: the user denied
 * notifications in the browser. `off` and `on`: this browser is not or is registered for this account.
 */
export type PushState = 'unsupported' | 'blocked' | 'off' | 'on' | 'working'

const SW_URL = '/sw.js'

/** Maps each subscription's endpoint to the ID the server gave it, so it can be deleted later. */
const STORE_KEY = 'postit.push.endpoints'

export function pushSupported(): boolean {
  return 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window
}

function readIds(): Record<string, string> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')
    return raw && typeof raw === 'object' ? (raw as Record<string, string>) : {}
  } catch {
    return {}
  }
}

function writeIds(ids: Record<string, string>) {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(ids))
  } catch {
    /* storage blocked: the switch reads as off after a reload and a new registration replaces the row */
  }
}

function remember(endpoint: string, id: string) {
  writeIds({ ...readIds(), [endpoint]: id })
}

function forget(endpoint: string) {
  const ids = readIds()
  if (!(endpoint in ids)) return
  writeIds(Object.fromEntries(Object.entries(ids).filter(([key]) => key !== endpoint)))
}

/** "Chrome on Windows" from the user agent; "Browser" when the browser is not recognized. */
export function deviceLabel(ua: string = navigator.userAgent): string {
  // Order matters: Edge, Opera, and Chrome all say "Chrome"; Chrome and Edge also say "Safari".
  const browsers: [RegExp, string][] = [
    [/Edg(e|A|iOS)?\//, 'Edge'],
    [/OPR\/|Opera/, 'Opera'],
    [/Firefox\/|FxiOS\//, 'Firefox'],
    [/Chrome\/|CriOS\//, 'Chrome'],
    [/Safari\//, 'Safari'],
  ]
  const systems: [RegExp, string][] = [
    [/Android/, 'Android'],
    [/iPhone|iPad|iPod/, 'iOS'],
    [/Windows/, 'Windows'],
    [/Mac OS X|Macintosh/, 'macOS'],
    [/CrOS/, 'ChromeOS'],
    [/Linux|X11/, 'Linux'],
  ]
  const browser = browsers.find(([re]) => re.test(ua))?.[1]
  const system = systems.find(([re]) => re.test(ua))?.[1]
  if (!browser) return 'Browser'
  return system ? `${browser} on ${system}` : browser
}

/** The VAPID public key (base64url) as the bytes `PushManager.subscribe` wants. */
function decodeKey(key: string): Uint8Array<ArrayBuffer> {
  const padded = key
    .replaceAll('-', '+')
    .replaceAll('_', '/')
    .padEnd(Math.ceil(key.length / 4) * 4, '=')
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0))
}

/** The registration with an active worker; registering again is harmless. */
async function activeRegistration(): Promise<ServiceWorkerRegistration> {
  await navigator.serviceWorker.register(SW_URL)
  // `ready` waits for an active worker; it would wait forever with no registration, hence the register above.
  return navigator.serviceWorker.ready
}

async function currentSubscription(): Promise<PushSubscription | null> {
  const registration = await navigator.serviceWorker.getRegistration(SW_URL)
  return (await registration?.pushManager.getSubscription()) ?? null
}

/**
 * Removes this browser's push registration for the signed-in account: deletes the server row, then
 * unsubscribes. Does nothing for a subscription this account did not register here. The browser
 * subscription ends even when the server call fails, and that failure is then thrown.
 */
export async function disablePush(api: ApiClient): Promise<void> {
  if (!pushSupported()) return
  const subscription = await currentSubscription()
  const id = subscription ? readIds()[subscription.endpoint] : undefined
  if (!subscription || !id) return
  let failure: unknown = null
  try {
    await ensureOk(api.DELETE('/api/v1/me/push-endpoints/{id}', { params: { path: { id } } }))
  } catch (e) {
    // Gone already (the server drops an endpoint the push service reports as expired).
    if (!(e instanceof ApiError && e.status === 404)) failure = e
  }
  try {
    await subscription.unsubscribe()
  } catch (e) {
    failure ??= e
  }
  forget(subscription.endpoint)
  if (failure !== null) throw failure as Error
}

const vapidQuery = (api: ApiClient) => ({
  queryKey: ['push', 'vapid-public-key'],
  queryFn: () => unwrap(api.GET('/api/v1/push/vapid-public-key')),
  staleTime: Infinity,
  retry: false,
})

function initialState(): PushState {
  if (!pushSupported()) return 'unsupported'
  return Notification.permission === 'denied' ? 'blocked' : 'working'
}

export interface BrowserPush {
  state: PushState
  /** True when `unsupported` is because the server has push off, not because of the browser. */
  serverOff: boolean
  /** The last failed enable or disable. */
  error: unknown
  enable: () => Promise<void>
  disable: () => Promise<void>
}

/** This browser's Web Push registration for the signed-in account. */
export function useBrowserPush(): BrowserPush {
  const api = useApi()
  const queryClient = useQueryClient()
  const supported = pushSupported()
  const [local, setLocal] = useState<PushState>(initialState)
  const [error, setError] = useState<unknown>(null)

  // Asked up front so the switch is never offered on a server that has push off (404 `push_disabled`).
  const vapid = useQuery({ ...vapidQuery(api), enabled: supported })
  const serverOff = vapid.error instanceof ApiError && vapid.error.code === 'push_disabled'

  useEffect(() => {
    if (!supported) return
    const run = { cancelled: false }
    void (async () => {
      let on = false
      try {
        const subscription = await currentSubscription()
        on = subscription !== null && subscription.endpoint in readIds()
      } catch {
        /* no readable registration: off */
      }
      if (!run.cancelled) setLocal(Notification.permission === 'denied' ? 'blocked' : on ? 'on' : 'off')
    })()
    return () => {
      run.cancelled = true
    }
  }, [supported])

  const enable = useCallback(async () => {
    if (!pushSupported()) return
    setError(null)
    // Never prompt a browser that already said no: it cannot answer, and a prompt loop would follow.
    if (Notification.permission === 'denied') {
      setLocal('blocked')
      return
    }
    setLocal('working')
    let created: PushSubscription | undefined
    try {
      if (Notification.permission !== 'granted') {
        const answer = await Notification.requestPermission()
        if (answer !== 'granted') {
          setLocal(answer === 'denied' ? 'blocked' : 'off')
          return
        }
      }
      const { key } = await queryClient.query(vapidQuery(api))
      const registration = await activeRegistration()
      const existing = await registration.pushManager.getSubscription()
      const subscription =
        existing ??
        (await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: decodeKey(key) }))
      if (!existing) created = subscription
      const keys = subscription.toJSON().keys
      const { id } = await unwrap(
        api.POST('/api/v1/me/push-endpoints', {
          body: {
            endpoint: subscription.endpoint,
            p256dh: keys?.p256dh ?? '',
            auth: keys?.auth ?? '',
            platform: 'web',
            device_label: deviceLabel(),
          },
        }),
      )
      remember(subscription.endpoint, id)
      setLocal('on')
    } catch (e) {
      // A subscription the server never accepted would only receive nothing; leave none behind.
      await created?.unsubscribe().catch(() => undefined)
      setError(e)
      setLocal('off')
    }
  }, [api, queryClient])

  const disable = useCallback(async () => {
    setError(null)
    setLocal('working')
    try {
      await disablePush(api)
    } catch (e) {
      setError(e)
    }
    setLocal('off')
  }, [api])

  return { state: serverOff ? 'unsupported' : local, serverOff, error, enable, disable }
}
