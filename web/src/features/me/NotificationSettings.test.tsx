import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { onTestFinished, vi } from 'vitest'
import { setLocale } from '@/i18n'
import arMe from '@/i18n/ar/me.json'
import enMe from '@/i18n/en-US/me.json'
import { server } from '@/test/server'
import { API, preferencesFixture } from '@/test/fixtures'
import { ENDPOINT, STORE_KEY, stubPush, type PushEnv } from '@/test/push'
import { renderApp } from '@/test/render'
import { NotificationSettings } from './NotificationSettings'

// A 65-byte uncompressed P-256 point in base64url (the first byte is 4).
const VAPID = btoa(String.fromCharCode(4, ...Array.from({ length: 64 }, (_, i) => i)))
  .replaceAll('+', '-')
  .replaceAll('/', '_')
  .replaceAll('=', '')
const ID = '018f0000-0000-7000-8000-0000000000e1'
const CHROME_ON_WINDOWS =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'

let env: PushEnv | null = null
afterEach(async () => {
  env?.restore()
  env = null
  vi.restoreAllMocks()
  await setLocale('en-US')
})

function mockApi(prefs = preferencesFixture()) {
  const updates = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json(prefs)),
    http.put(`${API}/api/v1/me/preferences`, async ({ request }) => {
      const body = (await request.json()) as Record<string, unknown>
      updates(body)
      return HttpResponse.json({ ...prefs, ...body, switches: prefs.switches })
    }),
    http.get(`${API}/api/v1/push/vapid-public-key`, () => HttpResponse.json({ key: VAPID })),
  )
  return updates
}

const pushSwitch = () => screen.findByRole('switch', { name: enMe.notifications.push.label })

describe('browser push', () => {
  it('blocked_permission_shows_blocked_state', async () => {
    env = stubPush({ permission: 'denied' })
    mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    expect(await screen.findByText(enMe.notifications.push.blocked)).toBeInTheDocument()
    const s = await pushSwitch()
    expect(s).toBeDisabled()
    await user.click(s)
    expect(env.notification.requestPermission).not.toHaveBeenCalled()
    expect(env.subscribe).not.toHaveBeenCalled()
  })

  it('does not prompt again when permission was blocked after the page loaded', async () => {
    env = stubPush({ permission: 'default' })
    mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeEnabled()
    })
    env.notification.permission = 'denied'
    await user.click(s)
    expect(await screen.findByText(enMe.notifications.push.blocked)).toBeInTheDocument()
    expect(env.notification.requestPermission).not.toHaveBeenCalled()
  })

  it('a dismissed prompt leaves it off without a second prompt', async () => {
    env = stubPush({ ask: 'default' })
    mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeEnabled()
    })
    await user.click(s)
    await waitFor(() => {
      expect(env!.notification.requestPermission).toHaveBeenCalledTimes(1)
    })
    expect(await screen.findByText(enMe.notifications.push.off)).toBeInTheDocument()
    expect(env.subscribe).not.toHaveBeenCalled()
  })

  it('enable_subscribes_and_registers', async () => {
    env = stubPush({ userAgent: CHROME_ON_WINDOWS })
    mockApi()
    const registered = vi.fn()
    server.use(
      http.post(`${API}/api/v1/me/push-endpoints`, async ({ request }) => {
        registered(await request.json())
        return HttpResponse.json({ id: ID }, { status: 201 })
      }),
    )
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeEnabled()
    })
    await user.click(s)
    await waitFor(() => {
      expect(s).toBeChecked()
    })
    expect(env.notification.requestPermission).toHaveBeenCalledTimes(1)
    const options = env.subscribe.mock.calls[0]?.[0] as { userVisibleOnly: boolean; applicationServerKey: Uint8Array }
    expect(options.userVisibleOnly).toBe(true)
    expect(options.applicationServerKey).toBeInstanceOf(Uint8Array)
    expect(options.applicationServerKey).toHaveLength(65)
    expect(registered).toHaveBeenCalledWith({
      endpoint: ENDPOINT,
      p256dh: 'BP256dhKey',
      auth: 'AuthSecret',
      platform: 'web',
      device_label: 'Chrome on Windows',
    })
    expect(JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')).toEqual({ [ENDPOINT]: ID })
  })

  it('a failed registration leaves no browser subscription behind', async () => {
    env = stubPush()
    mockApi()
    server.use(
      http.post(`${API}/api/v1/me/push-endpoints`, () =>
        HttpResponse.json({ code: 'push_endpoint_invalid', status: 422, title: 'x' }, { status: 422 }),
      ),
    )
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeEnabled()
    })
    await user.click(s)
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(env.subscription.unsubscribe).toHaveBeenCalled()
    expect(s).not.toBeChecked()
    expect(localStorage.getItem(STORE_KEY)).toBeNull()
  })

  it('disable_unsubscribes_and_deletes', async () => {
    env = stubPush({ permission: 'granted', existing: true })
    localStorage.setItem(STORE_KEY, JSON.stringify({ [ENDPOINT]: ID }))
    mockApi()
    const deleted = vi.fn()
    server.use(
      http.delete(`${API}/api/v1/me/push-endpoints/:id`, ({ params }) => {
        deleted(params.id)
        return new HttpResponse(null, { status: 204 })
      }),
    )
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeChecked()
    })
    await user.click(s)
    await waitFor(() => {
      expect(s).not.toBeChecked()
    })
    expect(deleted).toHaveBeenCalledWith(ID)
    expect(env.subscription.unsubscribe).toHaveBeenCalled()
    expect(JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')).toEqual({})
  })

  it('disable still unsubscribes when the stored ID is gone', async () => {
    env = stubPush({ permission: 'granted', existing: true })
    localStorage.setItem(STORE_KEY, JSON.stringify({ [ENDPOINT]: ID }))
    mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeChecked()
    })
    localStorage.clear()
    await user.click(s)
    await waitFor(() => {
      expect(s).not.toBeChecked()
    })
    expect(env.subscription.unsubscribe).toHaveBeenCalled()
  })

  it('a subscription this account never registered shows as off', async () => {
    env = stubPush({ permission: 'granted', existing: true })
    mockApi()
    renderApp(<NotificationSettings />)
    const s = await pushSwitch()
    await waitFor(() => {
      expect(s).toBeEnabled()
    })
    expect(s).not.toBeChecked()
  })

  it('push_disabled_server_shows_unsupported', async () => {
    env = stubPush()
    mockApi()
    server.use(
      http.get(`${API}/api/v1/push/vapid-public-key`, () =>
        HttpResponse.json({ code: 'push_disabled', status: 404, title: 'x' }, { status: 404 }),
      ),
    )
    renderApp(<NotificationSettings />)
    expect(await screen.findByText(enMe.notifications.push.serverOff)).toBeInTheDocument()
    expect(await pushSwitch()).toBeDisabled()
    expect(env.notification.requestPermission).not.toHaveBeenCalled()
  })

  it('shows unsupported where the browser has no Web Push', async () => {
    mockApi()
    renderApp(<NotificationSettings />)
    expect(await screen.findByText(enMe.notifications.push.unsupported)).toBeInTheDocument()
    expect(await pushSwitch()).toBeDisabled()
  })
})

describe('preferences', () => {
  it('timezone_prefilled_from_browser_when_default', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      timeZone: 'Asia/Riyadh',
    } as Intl.ResolvedDateTimeFormatOptions)
    const updates = mockApi(preferencesFixture({ timezone: 'UTC' }))
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const zone = await screen.findByRole('combobox', { name: enMe.notifications.timezone.label })
    expect(zone).toHaveTextContent('Asia/Riyadh')
    expect(updates).not.toHaveBeenCalled()
    await user.click(
      screen.getByRole('button', { name: enMe.notifications.timezone.save.replace('{{zone}}', 'Asia/Riyadh') }),
    )
    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ timezone: 'Asia/Riyadh' })
    })
  })

  it('falls back to the zones in use when the browser cannot list them', async () => {
    const original = Intl.supportedValuesOf
    Object.defineProperty(Intl, 'supportedValuesOf', { configurable: true, value: undefined })
    onTestFinished(() => {
      Object.defineProperty(Intl, 'supportedValuesOf', { configurable: true, value: original })
    })
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      timeZone: 'Asia/Calcutta',
    } as Intl.ResolvedDateTimeFormatOptions)
    mockApi(preferencesFixture({ timezone: 'Europe/Paris' }))
    renderApp(<NotificationSettings />)
    const zone = await screen.findByRole('combobox', { name: enMe.notifications.timezone.label })
    expect(zone).toHaveTextContent('Europe/Paris')
  })

  it('shows a detected zone the engine list leaves out', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      timeZone: 'Asia/Calcutta',
    } as Intl.ResolvedDateTimeFormatOptions)
    mockApi(preferencesFixture({ timezone: 'UTC' }))
    renderApp(<NotificationSettings />)
    const zone = await screen.findByRole('combobox', { name: enMe.notifications.timezone.label })
    expect(zone).toHaveTextContent('Asia/Calcutta')
  })

  it('keeps a stored zone and saves a changed one', async () => {
    const updates = mockApi(preferencesFixture({ timezone: 'Asia/Kolkata' }))
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    const zone = await screen.findByRole('combobox', { name: enMe.notifications.timezone.label })
    expect(zone).toHaveTextContent('Asia/Kolkata')
    await user.click(zone)
    await user.click(await screen.findByRole('option', { name: 'Europe/Paris' }))
    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ timezone: 'Europe/Paris' })
    })
  })

  it('kind_switch_sends_partial_update', async () => {
    const updates = mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    await user.click(await screen.findByRole('switch', { name: 'Push for Post failed' }))
    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ switches: [{ kind: 'delivery_failed', push: false }] })
    })
    await user.click(screen.getByRole('switch', { name: 'Email for Post partly published' }))
    await waitFor(() => {
      expect(updates).toHaveBeenLastCalledWith({ switches: [{ kind: 'job_partial', email: false }] })
    })
  })

  it('offers an email switch only for kinds the API says can be emailed', async () => {
    mockApi()
    renderApp(<NotificationSettings />)
    await screen.findByRole('switch', { name: 'Push for New comments' })
    expect(screen.getAllByRole('switch', { name: /^Email for / })).toHaveLength(2)
    expect(screen.queryByRole('switch', { name: 'Email for New comments' })).not.toBeInTheDocument()
  })

  it('master email and digest switches send partial updates', async () => {
    const updates = mockApi()
    const user = userEvent.setup()
    renderApp(<NotificationSettings />)
    await user.click(await screen.findByRole('switch', { name: enMe.notifications.email.label }))
    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ email_notifications: false })
    })
    await user.click(screen.getByRole('switch', { name: enMe.notifications.digest.label }))
    await waitFor(() => {
      expect(updates).toHaveBeenLastCalledWith({ comment_digest: true })
    })
  })

  it('tolerates a kind the catalog does not know yet', async () => {
    const prefs = preferencesFixture()
    prefs.switches = [...prefs.switches, { kind: 'brand_new_kind' as never, push: true, email: null }]
    mockApi(prefs)
    renderApp(<NotificationSettings />)
    const table = await screen.findByRole('table', { name: enMe.notifications.kinds.table })
    expect(within(table).getByText('brand_new_kind')).toBeInTheDocument()
  })
})

it('renders_rtl_in_arabic', async () => {
  env = stubPush({ permission: 'denied' })
  mockApi()
  await setLocale('ar')
  renderApp(<NotificationSettings />)
  expect(document.documentElement.dir).toBe('rtl')
  expect(await screen.findByRole('heading', { name: arMe.notifications.title })).toBeInTheDocument()
  expect(screen.getByRole('switch', { name: arMe.notifications.push.label })).toBeInTheDocument()
  expect(screen.getByText(arMe.notifications.push.blocked)).toBeInTheDocument()
  const table = await screen.findByRole('table', { name: arMe.notifications.kinds.table })
  expect(within(table).getByText(arMe.notifications.kinds.names.delivery_failed)).toBeInTheDocument()
  expect(
    screen.getByRole('switch', {
      name: arMe.notifications.kinds.pushFor.replace('{{kind}}', arMe.notifications.kinds.names.delivery_failed),
    }),
  ).toBeInTheDocument()
  // The switch thumb travels the other way in right-to-left.
  const thumb = screen.getAllByRole('switch')[0].querySelector('[data-slot="switch-thumb"]')
  expect(thumb?.className).toContain('rtl:')
})
