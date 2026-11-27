import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes, useLocation } from 'react-router'
import { vi } from 'vitest'
import { API, meFixture, platformFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { AccountsPage } from '../AccountsPage'
import { ConnectPage } from './ConnectPage'
import { DiscoveryChecklist } from './DiscoveryChecklist'
import { loadPending, PENDING_KEY, savePending } from './pendingStore'

const ME = '00000000-0000-7000-8000-000000000001'

const credentials = platformFixture('p1', 'Pixelfed', {
  connect: {
    method: 'credentials',
    schema: {
      type: 'object',
      properties: {
        host: { type: 'string', 'x-postit-label': { key: 'p1.host', en: 'Server' } },
        token: { type: 'string', 'x-postit-secret': true, 'x-postit-label': { key: 'p1.token', en: 'Access token' } },
      },
      required: ['host', 'token'],
      additionalProperties: false,
    } as never,
  },
  guide: {
    steps: [
      { key: 'p1.guide.1', en: 'Open your account settings.' },
      { key: 'p1.guide.2', en: 'Create an access token.' },
    ],
    url: 'https://help.example/pixelfed',
  },
})
const oauth = platformFixture('p2', 'Chirper', { connect: { method: 'oauth' } })

const discovered = [
  { remote_id: 'r1', display_name: 'Shop', handle: '@shop' },
  { remote_id: 'r2', display_name: 'Blog', handle: null },
]
const FUTURE = '2999-01-01T00:10:00Z'

function Where() {
  const location = useLocation()
  const { pathname } = location
  const state: unknown = location.state
  return (
    <p data-testid="where">
      {pathname} {(state as { notice?: string } | null)?.notice ?? ''}
    </p>
  )
}

function app() {
  return (
    <Routes>
      <Route path="/accounts" element={<Where />} />
      <Route path="/accounts/connect" element={<ConnectPage />} />
      <Route path="/accounts/connect/:platform" element={<ConnectPage />} />
    </Routes>
  )
}

function mock() {
  const calls = {
    credentials: vi.fn(),
    oauth: vi.fn(),
    confirm: vi.fn(),
    cancel: vi.fn(),
  }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/platforms`, () => HttpResponse.json([credentials, oauth])),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    http.post(`${API}/api/v1/accounts/credentials/p1`, async ({ request }) => {
      calls.credentials(await request.json())
      return HttpResponse.json({ connect_id: 'cid-1', pending_id: 'pend-1', expires_at: FUTURE, discovered })
    }),
    http.post(`${API}/api/v1/accounts/connect/p2`, async ({ request }) => {
      calls.oauth(await request.json())
      return HttpResponse.json({ authorize_url: 'https://chirper.example/authorize?state=s', expires_at: FUTURE })
    }),
    http.post(`${API}/api/v1/accounts/connect/:cid/confirm`, async ({ request }) => {
      calls.confirm(await request.json())
      return HttpResponse.json({ connected: [{ id: 'a1' }], skipped_elsewhere: 1, unknown: [] })
    }),
    http.delete(`${API}/api/v1/accounts/pending-connections/:id`, ({ params }) => {
      calls.cancel(params.id)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return calls
}

async function fillAndCheck(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByLabelText(/Server/), 'pix.example')
  await user.type(screen.getByLabelText(/Access token/), 'tok-123')
  await user.click(screen.getByRole('button', { name: 'Check credentials' }))
}

afterEach(() => {
  sessionStorage.clear()
})

it('credentials form renders from the schema with guide', async () => {
  mock()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  expect(await screen.findByLabelText(/Server/)).toBeInTheDocument()
  expect(screen.getByLabelText(/Access token/)).toHaveAttribute('type', 'password')
  const steps = screen.getAllByRole('listitem').map((li) => li.textContent)
  expect(steps).toEqual(['Open your account settings.', 'Create an access token.'])
  expect(screen.getByRole('link', { name: 'More help' })).toHaveAttribute('href', 'https://help.example/pixelfed')
})

it('lists the platforms with how each connects', async () => {
  mock()
  renderApp(null, { routes: app(), route: '/accounts/connect' })
  expect(await screen.findByText('Paste your credentials')).toBeInTheDocument()
  expect(screen.getByText('Sign in on the platform')).toBeInTheDocument()
  const links = screen.getAllByRole('link', { name: 'Connect' })
  expect(links[0]).toHaveAttribute('href', '/accounts/connect/p1')
})

it('violations map to fields', async () => {
  mock()
  server.use(
    http.post(`${API}/api/v1/accounts/credentials/p1`, () =>
      HttpResponse.json(
        {
          code: 'validation_failed',
          status: 422,
          title: 'x',
          violations: [
            { code: 'option_required', path: 'options.host' },
            { code: 'option_too_long', path: 'options.token', params: { max: 5, actual: 9 } },
          ],
        },
        { status: 422 },
      ),
    ),
  )
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  const alerts = await screen.findAllByRole('alert')
  expect(alerts.length).toBe(2)
  expect(screen.getByLabelText(/Server/)).toBeInvalid()
  expect(screen.getByLabelText(/Access token/)).toBeInvalid()
})

it('credentials_invalid marks the field', async () => {
  mock()
  server.use(
    http.post(`${API}/api/v1/accounts/credentials/p1`, () =>
      HttpResponse.json({ code: 'credentials_invalid', status: 422, title: 'x', field: 'token' }, { status: 422 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  expect(await screen.findByText('The platform rejected this. Check it and try again.')).toBeInTheDocument()
  expect(screen.getByLabelText(/Access token/)).toBeInvalid()
})

it('checklist confirms the chosen accounts', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  expect(calls.credentials).toHaveBeenCalledWith({ host: 'pix.example', token: 'tok-123' })
  const shop = await screen.findByRole('checkbox', { name: /Shop/ })
  expect(shop).toBeChecked()
  expect(screen.getByRole('checkbox', { name: /Blog/ })).toBeChecked()
  await user.click(screen.getByRole('checkbox', { name: /Blog/ }))
  await user.click(screen.getByRole('button', { name: 'Confirm' }))

  await waitFor(() => {
    expect(calls.confirm).toHaveBeenCalledWith({ account_ids: ['r1'] })
  })
  const where = await screen.findByTestId('where')
  expect(where).toHaveTextContent('/accounts')
  expect(where).toHaveTextContent('1 account connected.')
  expect(where).toHaveTextContent('1 account was skipped')
  expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
})

it('cancel deletes the pending row', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  await user.click(await screen.findByRole('button', { name: 'Cancel' }))
  await waitFor(() => {
    expect(calls.cancel).toHaveBeenCalledWith('pend-1')
  })
  expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
})

it('credential check with no pending_id cancels locally', async () => {
  const calls = mock()
  server.use(
    http.post(`${API}/api/v1/accounts/credentials/p1`, () =>
      HttpResponse.json({ connect_id: 'cid-1', expires_at: FUTURE, discovered }),
    ),
  )
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  await user.click(await screen.findByRole('button', { name: 'Cancel' }))
  expect((await screen.findByTestId('where')).textContent).toContain('/accounts')
  expect(calls.cancel).not.toHaveBeenCalled()
})

it('a credential that reaches no account says so and shows no checklist', async () => {
  mock()
  server.use(http.post(`${API}/api/v1/accounts/credentials/p1`, () => HttpResponse.json({ discovered: [] })))
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  expect(await screen.findByText('These credentials work, but they reach no account.')).toBeInTheDocument()
  expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
})

it('connect_id never written to localStorage', async () => {
  mock()
  const set = vi.spyOn(Storage.prototype, 'setItem')
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/accounts/connect/p1' })
  await fillAndCheck(user)
  await screen.findByRole('checkbox', { name: /Shop/ })
  // Held for ten minutes in this tab's session storage, and nowhere else.
  expect(sessionStorage.getItem(PENDING_KEY)).toContain('cid-1')
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i) ?? ''
    expect(localStorage.getItem(key) ?? '', key).not.toContain('cid-1')
  }
  const writes = set.mock.instances.map((target, i) => [target, set.mock.calls[i]] as const)
  for (const [target, [, value]] of writes) {
    if (target === localStorage) expect(value).not.toContain('cid-1')
  }
  set.mockRestore()
})

describe('oauth start', () => {
  const original = window.location
  afterEach(() => {
    Object.defineProperty(window, 'location', { value: original, writable: true, configurable: true })
  })

  it('oauth start sends return_to and navigates', async () => {
    const calls = mock()
    const assign = vi.fn()
    Object.defineProperty(window, 'location', {
      value: {
        origin: 'https://postit.test',
        pathname: '/accounts/connect/p2',
        search: '',
        href: 'https://postit.test/',
        assign,
      },
      writable: true,
      configurable: true,
    })
    const user = userEvent.setup()
    renderApp(null, { routes: app(), route: '/accounts/connect/p2' })
    await user.click(await screen.findByRole('button', { name: 'Continue to Chirper' }))
    await waitFor(() => {
      expect(calls.oauth).toHaveBeenCalledWith({ return_to: 'https://postit.test/oauth-done' })
    })
    await waitFor(() => {
      expect(assign).toHaveBeenCalledWith('https://chirper.example/authorize?state=s')
    })
  })
})

describe('resuming an unfinished connection', () => {
  const view = (over: Record<string, unknown> = {}) => ({
    connect: 'credentials',
    discovered,
    expires_at: FUTURE,
    pending_id: 'pend-1',
    platform: 'p1',
    ...over,
  })

  function accountsPage() {
    server.use(
      http.get(`${API}/api/v1/accounts`, () => HttpResponse.json({ data: [], page: 1, page_size: 20, total: 0 })),
      http.get(`${API}/api/v1/accounts/pending-connections`, () => HttpResponse.json([])),
    )
  }

  it('reload within 10 minutes resumes the checklist', async () => {
    const calls = mock()
    accountsPage()
    server.use(http.get(`${API}/api/v1/accounts/connect/cid-1`, () => HttpResponse.json(view())))
    savePending({ connectId: 'cid-1', pendingId: 'pend-1', platform: 'p1', userId: ME, expiresAt: FUTURE })
    renderApp(<AccountsPage />)
    expect(await screen.findByRole('checkbox', { name: /Shop/ })).toBeInTheDocument()
    expect(calls.confirm).not.toHaveBeenCalled()
  })

  it('expired entry is dropped', async () => {
    mock()
    accountsPage()
    savePending({
      connectId: 'cid-1',
      pendingId: 'pend-1',
      platform: 'p1',
      userId: ME,
      expiresAt: '2020-01-01T00:00:00Z',
    })
    renderApp(<AccountsPage />)
    expect(await screen.findByText('No accounts yet.')).toBeInTheDocument()
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
    expect(loadPending(ME)).toBeNull()
  })

  it("another user's entry is ignored", async () => {
    mock()
    accountsPage()
    savePending({ connectId: 'cid-1', pendingId: 'pend-1', platform: 'p1', userId: 'someone-else', expiresAt: FUTURE })
    renderApp(<AccountsPage />)
    expect(await screen.findByText('No accounts yet.')).toBeInTheDocument()
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
  })

  it('a connection the server no longer knows is dropped', async () => {
    mock()
    accountsPage()
    server.use(
      http.get(`${API}/api/v1/accounts/connect/cid-1`, () =>
        HttpResponse.json({ code: 'not_found', status: 404, title: 'x' }, { status: 404 }),
      ),
    )
    savePending({ connectId: 'cid-1', pendingId: 'pend-1', platform: 'p1', userId: ME, expiresAt: FUTURE })
    renderApp(<AccountsPage />)
    expect(await screen.findByText('No accounts yet.')).toBeInTheDocument()
    await waitFor(() => {
      expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
    })
  })
})

describe('the checklist clock', () => {
  it('shows the minutes left', async () => {
    mock()
    renderApp(
      <DiscoveryChecklist
        connectId="cid-1"
        pendingId="pend-1"
        discovered={discovered}
        expiresAt={new Date(Date.now() + 9 * 60_000 + 30_000).toISOString()}
        onDone={() => undefined}
      />,
    )
    expect(await screen.findByText(/10 minutes left to confirm/)).toBeInTheDocument()
  })

  it('closes with a message once the time has run out', async () => {
    mock()
    const done = vi.fn()
    const user = userEvent.setup()
    renderApp(
      <DiscoveryChecklist
        connectId="cid-1"
        pendingId="pend-1"
        discovered={discovered}
        expiresAt="2020-01-01T00:00:00Z"
        onDone={done}
      />,
    )
    expect(await screen.findByText('This connection expired. Start again.')).toBeInTheDocument()
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Start again' }))
    expect(done).toHaveBeenCalledWith('expired')
  })
})
