import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useState } from 'react'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap, type ApiClient } from '@/api/client'
import { ApiError } from '@/api/errors'
import { useMe } from './useMe'

/**
 * `unsupported`: no Web Push in this browser, or the server has push off. `blocked`: the user denied
 * notifications in the browser. `off` and `on`: this browser is not or is registered for this account.
 */
export type PushState = 'unsupported' | 'blocked' | 'off' | 'on' | 'working'

const SW_URL = '/sw.js'

/**
 * Maps each subscription's endpoint to the ID the server gave it and the user it was registered for,
 * so it can be deleted later and a shared browser never reads another account's registration as this
 * account's.
 */
const STORE_KEY = 'postit.push.endpoints'

interface Stored {
  id: string
  user: string
}

export function pushSupported(): boolean {
  return 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window
}

function isStored(value: unknown): value is Stored {
  if (!value || typeof value !== 'object') return false
  const entry = value as Record<string, unknown>
  return typeof entry.id === 'string' && typeof entry.user === 'string'
}

function readIds(): Partial<Record<string, Stored>> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')
    if (!raw || typeof raw !== 'object') return {}
    // An entry without its user (an older format) belongs to nobody: it reads as off.
    return Object.fromEntries(Object.entries(raw).filter(([, value]) => isStored(value)))
  } catch {
    return {}
  }
}

/** The server ID of `endpoint` when `user` registered it; undefined for anyone else's. */
function ownedId(endpoint: string, user: string | undefined): string | undefined {
  const entry = readIds()[endpoint]
  return user !== undefined && entry?.user === user ? entry.id : undefined
}

function writeIds(ids: Partial<Record<string, Stored>>) {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(ids))
  } catch {
    /* storage blocked: the switch reads as off after a reload and a new registration replaces the row */
  }
}

function remember(endpoint: string, id: string, user: string) {
  writeIds({ ...readIds(), [endpoint]: { id, user } })
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
 * Removes this browser's push registration for the signed-in account `user`: deletes the server row,
 * then unsubscribes. Without a stored ID for `user` (storage cleared or blocked, or another account's
 * registration) the server call is skipped and the browser subscription still ends; the server drops
 * the row on its first 410. The browser subscription also ends when the server call fails, and that
 * failure is then thrown.
 */
export async function disablePush(api: ApiClient, user: string | undefined): Promise<void> {
  if (!pushSupported()) return
  const subscription = await currentSubscription()
  if (!subscription) return
  const id = ownedId(subscription.endpoint, user)
  let failure: unknown = null
  try {
    if (id) await ensureOk(api.DELETE('/api/v1/me/push-endpoints/{id}', { params: { path: { id } } }))
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

/**
 * Ends this browser's push subscription when another account registered it, so the next person on a
 * shared browser never sees the previous account's notifications, even when that account's session
 * ended without the sign-out button. The previous account's server row cannot be deleted from here (it
 * is not this user's): the push service then answers 410 and the server drops it, and a registration
 * by this user re-homes the endpoint on the server anyway.
 */
export async function releaseForeignPush(user: string): Promise<void> {
  if (!pushSupported()) return
  try {
    const subscription = await currentSubscription()
    if (!subscription) return
    const entry = readIds()[subscription.endpoint]
    if (!entry || entry.user === user) return
    forget(subscription.endpoint)
    await subscription.unsubscribe()
  } catch {
    /* best effort: the server re-homes the endpoint when this user switches push on */
  }
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
  const userId = useMe().data?.id

  // Asked up front so the switch is never offered on a server that has push off (404 `push_disabled`).
  const vapid = useQuery({ ...vapidQuery(api), enabled: supported })
  const serverOff = vapid.error instanceof ApiError && vapid.error.code === 'push_disabled'

  useEffect(() => {
    // Waits for the signed-in user: a registration reads as on only when this user made it.
    if (!supported || userId === undefined) return
    const run = { cancelled: false }
    void (async () => {
      let on = false
      try {
        const subscription = await currentSubscription()
        on = subscription !== null && ownedId(subscription.endpoint, userId) !== undefined
      } catch {
        /* no readable registration: off */
      }
      if (!run.cancelled) setLocal(Notification.permission === 'denied' ? 'blocked' : on ? 'on' : 'off')
    })()
    return () => {
      run.cancelled = true
    }
  }, [supported, userId])

  const enable = useCallback(async () => {
    if (!pushSupported() || userId === undefined) return
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
      let existing = await registration.pushManager.getSubscription()
      // Reuses only this user's own subscription. Another account's (or one nobody claims) ends first,
      // so the new registration has a fresh endpoint the previous account never held.
      if (existing && ownedId(existing.endpoint, userId) === undefined) {
        forget(existing.endpoint)
        await existing.unsubscribe()
        existing = null
      }
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
      remember(subscription.endpoint, id, userId)
      setLocal('on')
    } catch (e) {
      // A subscription the server never accepted would only receive nothing; leave none behind.
      await created?.unsubscribe().catch(() => undefined)
      setError(e)
      setLocal('off')
    }
  }, [api, queryClient, userId])

  const disable = useCallback(async () => {
    setError(null)
    setLocal('working')
    try {
      await disablePush(api, userId)
    } catch (e) {
      setError(e)
    }
    setLocal('off')
  }, [api, userId])

  return { state: serverOff ? 'unsupported' : local, serverOff, error, enable, disable }
}
