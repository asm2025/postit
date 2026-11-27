import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import {
  accountFixture,
  API,
  delegatedWorkspaceFixture,
  meFixture,
  ownWorkspaceFixture,
  platformFixture,
} from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import type { Account, PendingConnection } from './queries'
import { AccountsPage } from './AccountsPage'

const OWNER = '0000000a-0000-7000-8000-00000000000a'

const page = <T,>(data: T[]) => ({ data, page: 1, page_size: 20, total: data.length })

function mock(accounts: Account[], pending: PendingConnection[] = []) {
  const calls = { list: vi.fn(), cancel: vi.fn(), pendingList: vi.fn() }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/platforms`, () =>
      HttpResponse.json([platformFixture('p1', 'Pixelfed'), platformFixture('p2', 'Mailer')]),
    ),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    http.get(`${API}/api/v1/accounts`, ({ request }) => {
      calls.list(new URL(request.url).searchParams.toString())
      return HttpResponse.json(page(accounts))
    }),
    http.get(`${API}/api/v1/accounts/pending-connections`, () => {
      calls.pendingList()
      return HttpResponse.json(pending)
    }),
    http.delete(`${API}/api/v1/accounts/pending-connections/:id`, ({ params }) => {
      calls.cancel(params.id)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return calls
}

const rows = [
  accountFixture({ id: 'a2', platform: 'p2', display_name: 'Newsletter' }),
  accountFixture({ id: 'a1', platform: 'p1', display_name: 'Shop', test_account: true }),
  accountFixture({
    id: 'a3',
    platform: 'p1',
    display_name: 'Old blog',
    status: 'needs_reconnect',
    last_checked_at: null,
  }),
]

it('owner sees connect and import', async () => {
  mock(rows)
  renderApp(<AccountsPage />)
  expect(await screen.findByRole('link', { name: 'Connect an account' })).toHaveAttribute('href', '/accounts/connect')
  expect(screen.getByRole('link', { name: 'Import from CSV' })).toHaveAttribute('href', '/accounts/import')
  expect(screen.getByRole('link', { name: 'Target groups' })).toHaveAttribute('href', '/accounts/groups')
})

it.each(['view', 'edit', 'publish'] as const)('delegate at %s sees no owner actions', async (level) => {
  const calls = mock(rows, [
    { pending_id: 'x', platform: 'p1', created_at: '2026-11-20T10:00:00Z', expires_at: '2026-11-20T10:10:00Z' },
  ])
  renderApp(<AccountsPage />, {
    actAs: OWNER,
    workspaces: [ownWorkspaceFixture(), delegatedWorkspaceFixture(OWNER, { level })],
  })
  expect(await screen.findByRole('link', { name: 'Shop' })).toBeInTheDocument()
  expect(screen.queryByRole('link', { name: 'Connect an account' })).not.toBeInTheDocument()
  expect(screen.queryByRole('link', { name: 'Import from CSV' })).not.toBeInTheDocument()
  // Pending connections belong to the owner: not even asked for.
  expect(calls.pendingList).not.toHaveBeenCalled()
  expect(screen.queryByText(/Unfinished connection/)).not.toBeInTheDocument()
})

it('lists accounts grouped by platform with status and test flag', async () => {
  mock(rows)
  renderApp(<AccountsPage />)
  const table = await screen.findByRole('table', { name: 'Accounts' })
  const names = within(table)
    .getAllByRole('row')
    .slice(1)
    .map((r) => within(r).getAllByRole('cell')[0].textContent)
  // Pixelfed accounts first (the platform order), then Mailer.
  expect(names).toEqual(['Shop', 'Old blog', 'Newsletter'])
  expect(within(table).getByText('Test')).toBeInTheDocument()
  expect(within(table).getByText('Needs reconnecting')).toBeInTheDocument()
})

it('show disconnected is in the URL and in the request', async () => {
  const calls = mock(rows)
  const user = userEvent.setup()
  renderApp(<AccountsPage />)
  await screen.findByRole('link', { name: 'Shop' })
  expect(calls.list).toHaveBeenLastCalledWith(expect.not.stringContaining('include_disconnected=true'))
  await user.click(screen.getByRole('switch', { name: 'Show disconnected' }))
  await waitFor(() => {
    expect(calls.list).toHaveBeenLastCalledWith(expect.stringContaining('include_disconnected=true'))
  })
})

it('pending connection shows cancel and start again, never confirm', async () => {
  const calls = mock(rows, [
    { pending_id: 'pend-1', platform: 'p1', created_at: '2026-11-20T10:00:00Z', expires_at: '2999-01-01T10:00:00Z' },
  ])
  const user = userEvent.setup()
  renderApp(<AccountsPage />)
  const row = (await screen.findByText(/Unfinished connection to Pixelfed/)).closest('li')
  if (!row) throw new Error('no pending row')
  expect(within(row).getByRole('link', { name: 'Start again' })).toHaveAttribute('href', '/accounts/connect/p1')
  expect(within(row).queryByRole('button', { name: /confirm/i })).not.toBeInTheDocument()
  await user.click(within(row).getByRole('button', { name: 'Cancel' }))
  await waitFor(() => {
    expect(calls.cancel).toHaveBeenCalledWith('pend-1')
  })
})

it('says so when there are no accounts', async () => {
  mock([])
  renderApp(<AccountsPage />)
  expect(await screen.findByText('No accounts yet.')).toBeInTheDocument()
})

describe('in a right-to-left language', () => {
  beforeEach(() => {
    document.documentElement.dir = 'rtl'
  })
  afterEach(() => {
    document.documentElement.dir = 'ltr'
  })

  it('rtl render keeps logical classes', async () => {
    mock(rows)
    const { container } = renderApp(<AccountsPage />)
    await screen.findByRole('link', { name: 'Shop' })
    expect(container.querySelector('[class*="ml-"], [class*="mr-"]')).toBeNull()
  })
})
