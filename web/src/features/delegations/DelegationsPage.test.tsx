import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { getActAs } from '@/workspace/store'
import { API, delegatedWorkspaceFixture, meFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import type { Delegation } from './queries'
import { DelegationsPage } from './DelegationsPage'

const ME = '00000000-0000-7000-8000-000000000001'
const DEE = '00000000-0000-7000-8000-0000000000de'
const OWNER = '0000000a-0000-7000-8000-00000000000a'
const WORKSPACES = [ownWorkspaceFixture(), delegatedWorkspaceFixture(OWNER)]

function grant(over: Partial<Delegation> = {}): Delegation {
  return {
    id: '018f0000-0000-7000-8000-0000000000d1',
    level: 'edit',
    all_accounts: true,
    account_ids: [],
    delegate: { id: DEE, display_name: 'Dee Delegate', status: 'active' },
    owner: { id: ME, display_name: 'Test User', status: 'active' },
    expires_at: null,
    created_at: '2026-11-01T10:00:00Z',
    updated_at: '2026-11-01T10:00:00Z',
    ...over,
  }
}

const page = <T,>(data: T[]) => ({ data, page: 1, page_size: 20, total: data.length })

function mock(granted: Delegation[] = [], received: Delegation[] = []) {
  const calls = { post: vi.fn(), patch: vi.fn(), del: vi.fn(), leave: vi.fn(), granted: vi.fn() }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/delegations`, () => {
      calls.granted()
      return HttpResponse.json(page(granted))
    }),
    http.get(`${API}/api/v1/delegations/received`, () => HttpResponse.json(page(received))),
    http.get(`${API}/api/v1/accounts`, () => HttpResponse.json(page([]))),
    http.post(`${API}/api/v1/delegations`, async ({ request }) => {
      calls.post(await request.json())
      return HttpResponse.json(grant(), { status: 201 })
    }),
    http.patch(`${API}/api/v1/delegations/:id`, async ({ request }) => {
      calls.patch(await request.json())
      return HttpResponse.json(grant())
    }),
    http.delete(`${API}/api/v1/delegations/received/:id`, () => {
      calls.leave()
      return new HttpResponse(null, { status: 204 })
    }),
    http.delete(`${API}/api/v1/delegations/:id`, () => {
      calls.del()
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return calls
}

function lookup(user: { id: string; display_name: string } | null) {
  server.use(http.get(`${API}/api/v1/users/lookup`, () => HttpResponse.json({ user })))
}

async function openGrantDialog(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole('button', { name: 'Give access' }))
  return screen.findByRole('dialog')
}

it('grant by exact email', async () => {
  const calls = mock()
  lookup({ id: DEE, display_name: 'Dee Delegate' })
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  const dialog = await openGrantDialog(user)

  await user.type(within(dialog).getByLabelText('Their email'), 'dee@postit.test')
  expect(await within(dialog).findByText(/Found:.*Dee Delegate/)).toBeInTheDocument()
  await user.click(within(dialog).getByRole('combobox', { name: 'Access level' }))
  await user.click(await screen.findByRole('option', { name: 'Publish' }))
  await user.click(within(dialog).getByRole('button', { name: 'Give access' }))

  await waitFor(() => {
    expect(calls.post).toHaveBeenCalledWith({ delegate_id: DEE, level: 'publish', expires_at: null })
  })
})

it('lookup miss shows the message and blocks submit', async () => {
  mock()
  lookup(null)
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  const dialog = await openGrantDialog(user)
  await user.type(within(dialog).getByLabelText('Their email'), 'nobody@postit.test')
  expect(await within(dialog).findByText('No postit user has this email')).toBeInTheDocument()
  expect(within(dialog).getByRole('button', { name: 'Give access' })).toBeDisabled()
})

it('rate_limited lookup shows the wait', async () => {
  mock()
  server.use(
    http.get(`${API}/api/v1/users/lookup`, () =>
      HttpResponse.json(
        { code: 'rate_limited', status: 429, title: 'Too many' },
        { status: 429, headers: { 'Retry-After': '30' } },
      ),
    ),
  )
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  const dialog = await openGrantDialog(user)
  await user.type(within(dialog).getByLabelText('Their email'), 'dee@postit.test')
  expect(await within(dialog).findByText('Too many lookups. Try again in 30 seconds.')).toBeInTheDocument()
})

it('edit keeps expires_at', async () => {
  const calls = mock([grant({ expires_at: '2026-12-31T20:59:00Z' })])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  await user.click(await screen.findByRole('button', { name: 'Edit access for Dee Delegate' }))
  const dialog = await screen.findByRole('dialog')
  await user.click(within(dialog).getByRole('combobox', { name: 'Access level' }))
  await user.click(await screen.findByRole('option', { name: 'Publish' }))
  await user.click(within(dialog).getByRole('button', { name: 'Save' }))

  await waitFor(() => {
    expect(calls.patch).toHaveBeenCalledWith({
      level: 'publish',
      all_accounts: true,
      expires_at: '2026-12-31T20:59:00Z',
    })
  })
})

it('edit clears expires_at only when the date is emptied', async () => {
  const calls = mock([grant({ expires_at: '2026-12-31T20:59:00Z' })])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  await user.click(await screen.findByRole('button', { name: 'Edit access for Dee Delegate' }))
  const dialog = await screen.findByRole('dialog')
  await user.clear(within(dialog).getByLabelText('Expires on'))
  await user.click(within(dialog).getByRole('button', { name: 'Save' }))
  await waitFor(() => {
    expect(calls.patch).toHaveBeenCalledWith(expect.objectContaining({ expires_at: null }))
  })
})

it('revoke asks first', async () => {
  const calls = mock([grant()])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  await user.click(await screen.findByRole('button', { name: 'Edit access for Dee Delegate' }))
  const dialog = await screen.findByRole('dialog')
  await user.click(within(dialog).getByRole('button', { name: 'Revoke access' }))

  const confirm = await screen.findByRole('alertdialog')
  expect(within(confirm).getByText('Revoke access?')).toBeInTheDocument()
  expect(calls.del).not.toHaveBeenCalled()
  await user.click(within(confirm).getByRole('button', { name: 'Revoke access' }))
  await waitFor(() => {
    expect(calls.del).toHaveBeenCalledOnce()
  })
})

it('open workspace switches to the owner', async () => {
  mock([], [grant({ owner: { id: OWNER, display_name: 'Olive Owner', status: 'active' } })])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />, { route: '/?tab=received', workspaces: WORKSPACES })
  await user.click(await screen.findByRole('button', { name: "Open Olive Owner's workspace" }))
  await waitFor(() => {
    expect(getActAs()).toBe(OWNER)
  })
})

it('leave the active workspace returns to own', async () => {
  const calls = mock([], [grant({ owner: { id: OWNER, display_name: 'Olive Owner', status: 'active' } })])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />, { actAs: OWNER, workspaces: WORKSPACES })
  await user.click(await screen.findByRole('button', { name: "Leave Olive Owner's workspace" }))
  const confirm = await screen.findByRole('alertdialog')
  await user.click(within(confirm).getByRole('button', { name: 'Leave' }))

  await waitFor(() => {
    expect(calls.leave).toHaveBeenCalledOnce()
  })
  await waitFor(() => {
    expect(getActAs()).toBeNull()
  })
})

it('while delegated only Received shows', async () => {
  mock([], [grant({ owner: { id: OWNER, display_name: 'Olive Owner', status: 'active' } })])
  renderApp(<DelegationsPage />, { actAs: OWNER, workspaces: WORKSPACES })
  expect(await screen.findByRole('table', { name: 'Received' })).toBeInTheDocument()
  expect(screen.queryByRole('tab')).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Give access' })).not.toBeInTheDocument()
})

it('tab=granted while delegated falls back to Received', async () => {
  const calls = mock([grant()], [grant({ owner: { id: OWNER, display_name: 'Olive Owner', status: 'active' } })])
  renderApp(<DelegationsPage />, { route: '/?tab=granted', actAs: OWNER, workspaces: WORKSPACES })
  expect(await screen.findByRole('table', { name: 'Received' })).toBeInTheDocument()
  expect(calls.granted).not.toHaveBeenCalled()
})

it('shows Granted first in the own workspace, with a Received tab', async () => {
  mock([grant()], [grant({ owner: { id: OWNER, display_name: 'Olive Owner', status: 'active' } })])
  const user = userEvent.setup()
  renderApp(<DelegationsPage />)
  expect(await screen.findByRole('table', { name: 'Granted' })).toBeInTheDocument()
  expect(screen.getByText('Dee Delegate')).toBeInTheDocument()
  await user.click(screen.getByRole('tab', { name: 'Received' }))
  expect(await screen.findByRole('table', { name: 'Received' })).toBeInTheDocument()
})
