import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes, useLocation } from 'react-router'
import { vi } from 'vitest'
import { API, delegatedWorkspaceFixture, meFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { AccountDetailPage } from './AccountDetailPage'
import type { Account, WritingStyle } from './queries'

const OWNER = '0000000a-0000-7000-8000-00000000000a'
const ID = '018f0000-0000-7000-8000-0000000000a1'

const account: Account = {
  id: ID,
  platform: 'p1',
  display_name: 'Shop',
  remote_account_id: 'remote-1',
  status: 'active',
  test_account: false,
  last_checked_at: '2026-11-20T10:00:00Z',
  created_at: '2026-10-01T10:00:00Z',
  updated_at: '2026-10-01T10:00:00Z',
}

const style: WritingStyle = {
  account_id: ID,
  id: null,
  tone: 'warm',
  emoji: 'light',
  hashtags: 'few',
  language: null,
  notes: '',
  updated_at: null,
}

function Where() {
  return <p data-testid="where">{useLocation().pathname}</p>
}

function app() {
  return (
    <Routes>
      <Route path="/accounts" element={<Where />} />
      <Route path="/accounts/:id" element={<AccountDetailPage />} />
    </Routes>
  )
}

function mock(over: { account?: Account | 'missing' } = {}) {
  const calls = { patch: vi.fn(), check: vi.fn(), put: vi.fn(), del: vi.fn() }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/platforms`, () =>
      HttpResponse.json([
        {
          platform: 'p1',
          display_name: 'Pixelfed',
          brand_color: '#336699',
          kind: 'broadcast',
          connect: { method: 'credentials' },
          guide: { steps: [] },
        },
      ]),
    ),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    http.get(`${API}/api/v1/accounts/:id`, () =>
      over.account === 'missing'
        ? HttpResponse.json({ code: 'not_found', status: 404, title: 'Not found' }, { status: 404 })
        : HttpResponse.json(over.account ?? account),
    ),
    http.get(`${API}/api/v1/accounts/:id/writing-style`, () => HttpResponse.json(style)),
    http.put(`${API}/api/v1/accounts/:id/writing-style`, async ({ request }) => {
      const body = (await request.json()) as Record<string, unknown>
      calls.put(body)
      return HttpResponse.json({ ...style, ...body })
    }),
    http.patch(`${API}/api/v1/accounts/:id`, async ({ request }) => {
      calls.patch(await request.json())
      return HttpResponse.json(account)
    }),
    http.post(`${API}/api/v1/accounts/:id/check`, () => {
      calls.check()
      return HttpResponse.json({ ...account, status: 'needs_reconnect' })
    }),
    http.delete(`${API}/api/v1/accounts/:id`, () => {
      calls.del()
      return HttpResponse.json({ canceled_deliveries: 2, native_scheduled_remaining: 0 })
    }),
  )
  return calls
}

const delegated = (level: 'view' | 'edit' | 'publish', limit?: string[]) => ({
  actAs: OWNER,
  workspaces: [
    ownWorkspaceFixture(),
    delegatedWorkspaceFixture(OWNER, limit ? { level, all_accounts: false, account_ids: limit } : { level }),
  ],
})

it('owner sees facts, manage actions, and the writing style', async () => {
  mock()
  renderApp(null, { routes: app(), route: `/accounts/${ID}` })
  expect(await screen.findByRole('heading', { name: 'Shop', level: 1 })).toBeInTheDocument()
  expect(screen.getByText('remote-1')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Check now' })).toBeInTheDocument()
  expect(screen.getByRole('switch', { name: /Test account/ })).toBeInTheDocument()
  expect(await screen.findByLabelText('Tone')).toHaveValue('warm')
})

it('owner renames and flags a test account', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: `/accounts/${ID}` })
  const name = await screen.findByLabelText('Name in postit')
  await user.clear(name)
  await user.type(name, 'Shop EU')
  await user.click(within(name.closest('div') as HTMLElement).getByRole('button', { name: 'Save' }))
  await waitFor(() => {
    expect(calls.patch).toHaveBeenCalledWith({ display_name: 'Shop EU' })
  })
  await user.click(screen.getByRole('switch', { name: /Test account/ }))
  await waitFor(() => {
    expect(calls.patch).toHaveBeenCalledWith({ test_account: true })
  })
})

it('check now shows the new status', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: `/accounts/${ID}` })
  await user.click(await screen.findByRole('button', { name: 'Check now' }))
  expect(await screen.findByText('The platform says: Needs reconnecting.')).toBeInTheDocument()
  expect(calls.check).toHaveBeenCalledOnce()
})

it.each(['view', 'edit', 'publish'] as const)('delegate at %s sees no owner actions', async (level) => {
  mock()
  renderApp(null, { routes: app(), route: `/accounts/${ID}`, ...delegated(level) })
  expect(await screen.findByRole('heading', { name: 'Shop', level: 1 })).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Check now' })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Disconnect' })).not.toBeInTheDocument()
  expect(screen.queryByLabelText('Name in postit')).not.toBeInTheDocument()
})

it('writing style read-only for view', async () => {
  mock()
  renderApp(null, { routes: app(), route: `/accounts/${ID}`, ...delegated('view') })
  expect(await screen.findByLabelText('Tone')).toBeDisabled()
  expect(screen.getByLabelText('Notes')).toBeDisabled()
  expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument()
})

it('writing style editable for edit in limit', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: `/accounts/${ID}`, ...delegated('edit', [ID]) })
  const tone = await screen.findByLabelText('Tone')
  expect(tone).toBeEnabled()
  await user.clear(tone)
  await user.type(tone, 'dry')
  await user.click(screen.getByRole('button', { name: 'Save' }))
  await waitFor(() => {
    expect(calls.put).toHaveBeenCalledWith({ tone: 'dry', emoji: 'light', hashtags: 'few', language: null, notes: '' })
  })
})

it('writing style stays read-only for an account outside a limited grant', async () => {
  mock()
  renderApp(null, { routes: app(), route: `/accounts/${ID}`, ...delegated('publish', ['some-other-account']) })
  expect(await screen.findByLabelText('Tone')).toBeDisabled()
})

it('an account outside the limit is not found', async () => {
  mock({ account: 'missing' })
  renderApp(null, { routes: app(), route: `/accounts/${ID}`, ...delegated('publish', ['x']) })
  expect(await screen.findByText('Account not found')).toBeInTheDocument()
  expect(screen.getByRole('link', { name: 'Back to accounts' })).toHaveAttribute('href', '/accounts')
})

it('disconnect asks first and returns to the list', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: `/accounts/${ID}` })
  await user.click(await screen.findByRole('button', { name: 'Disconnect' }))
  const confirm = await screen.findByRole('alertdialog')
  expect(within(confirm).getByText(/Scheduled deliveries to .*Shop.* will be canceled/)).toBeInTheDocument()
  expect(calls.del).not.toHaveBeenCalled()
  await user.click(within(confirm).getByRole('button', { name: 'Disconnect' }))
  await waitFor(() => {
    expect(calls.del).toHaveBeenCalledOnce()
  })
  expect(await screen.findByTestId('where')).toHaveTextContent('/accounts')
})

describe('in a right-to-left language', () => {
  beforeEach(() => {
    document.documentElement.dir = 'rtl'
  })
  afterEach(() => {
    document.documentElement.dir = 'ltr'
  })

  it('rtl render keeps logical classes', async () => {
    mock()
    const { container } = renderApp(null, { routes: app(), route: `/accounts/${ID}` })
    await screen.findByRole('heading', { name: 'Shop', level: 1 })
    expect(container.querySelector('[class*="ml-"], [class*="mr-"]')).toBeNull()
  })
})
