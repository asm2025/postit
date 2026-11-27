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
import { TargetGroupsPage } from './TargetGroupsPage'
import type { TargetGroup } from './queries'

const OWNER = '0000000a-0000-7000-8000-00000000000a'
const accounts = [
  accountFixture({ id: 'a1', display_name: 'Shop' }),
  accountFixture({ id: 'a2', display_name: 'Blog' }),
  accountFixture({ id: 'a3', display_name: 'Newsletter', platform: 'p2' }),
]
const group = (over: Partial<TargetGroup> = {}): TargetGroup => ({
  id: 'g1',
  name: 'Launch',
  account_ids: ['a1'],
  updated_at: '2026-11-20T10:00:00Z',
  ...over,
})

const page = <T,>(data: T[]) => ({ data, page: 1, page_size: 20, total: data.length })

function mock(groups: TargetGroup[] = [group()], visible = accounts) {
  const calls = { post: vi.fn(), put: vi.fn(), del: vi.fn() }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/platforms`, () =>
      HttpResponse.json([platformFixture('p1', 'Pixelfed'), platformFixture('p2', 'Mailer')]),
    ),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    http.get(`${API}/api/v1/accounts`, () => HttpResponse.json(page(visible))),
    http.get(`${API}/api/v1/target-groups`, () => HttpResponse.json(page(groups))),
    http.post(`${API}/api/v1/target-groups`, async ({ request }) => {
      calls.post(await request.json())
      return HttpResponse.json(group({ id: 'g2' }), { status: 201 })
    }),
    http.put(`${API}/api/v1/target-groups/:id`, async ({ request }) => {
      calls.put(await request.json())
      return HttpResponse.json(group())
    }),
    http.delete(`${API}/api/v1/target-groups/:id`, () => {
      calls.del()
      return new HttpResponse(null, { status: 204 })
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

it('shows each group with its accounts', async () => {
  mock([group({ account_ids: ['a1', 'a2'] }), group({ id: 'g3', name: 'Empty', account_ids: [] })])
  renderApp(<TargetGroupsPage />)
  const table = await screen.findByRole('table', { name: 'Target groups' })
  expect(within(table).getByText('Launch')).toBeInTheDocument()
  expect(await within(table).findByText('Shop')).toBeInTheDocument()
  expect(within(table).getByText('Blog')).toBeInTheDocument()
  expect(within(table).getByText('No accounts')).toBeInTheDocument()
})

it('create a group', async () => {
  const calls = mock([])
  const user = userEvent.setup()
  renderApp(<TargetGroupsPage />)
  await user.click(await screen.findByRole('button', { name: 'New group' }))
  const dialog = await screen.findByRole('dialog')
  await user.type(within(dialog).getByLabelText('Name'), 'Spring')
  await user.click(await within(dialog).findByRole('checkbox', { name: 'Shop' }))
  await user.click(within(dialog).getByRole('checkbox', { name: 'Newsletter' }))
  await user.click(within(dialog).getByRole('button', { name: 'Save group' }))
  await waitFor(() => {
    expect(calls.post).toHaveBeenCalledWith({ name: 'Spring', account_ids: ['a1', 'a3'] })
  })
})

it('edit replaces visible accounts', async () => {
  const calls = mock([group({ account_ids: ['a1'] })])
  const user = userEvent.setup()
  renderApp(<TargetGroupsPage />)
  await user.click(await screen.findByRole('button', { name: 'Edit group Launch' }))
  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByRole('checkbox', { name: 'Shop' })).toBeChecked()
  await user.click(within(dialog).getByRole('checkbox', { name: 'Shop' }))
  await user.click(within(dialog).getByRole('checkbox', { name: 'Blog' }))
  await user.click(within(dialog).getByRole('button', { name: 'Save group' }))
  await waitFor(() => {
    expect(calls.put).toHaveBeenCalledWith({ name: 'Launch', account_ids: ['a2'] })
  })
})

it('delete asks first', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(<TargetGroupsPage />)
  await user.click(await screen.findByRole('button', { name: 'Delete group Launch' }))
  const confirm = await screen.findByRole('alertdialog')
  expect(calls.del).not.toHaveBeenCalled()
  await user.click(within(confirm).getByRole('button', { name: 'Delete group' }))
  await waitFor(() => {
    expect(calls.del).toHaveBeenCalledOnce()
  })
})

it('view level sees no edit actions', async () => {
  mock()
  renderApp(<TargetGroupsPage />, delegated('view'))
  expect(await screen.findByText('Launch')).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'New group' })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: /Edit group/ })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: /Delete group/ })).not.toBeInTheDocument()
})

it('limited delegate picks only accounts in limit', async () => {
  // The server only lists the accounts the grant reaches.
  mock([group({ account_ids: ['a1'] })], [accounts[0]])
  const user = userEvent.setup()
  renderApp(<TargetGroupsPage />, delegated('edit', ['a1']))
  await user.click(await screen.findByRole('button', { name: 'New group' }))
  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByRole('checkbox', { name: 'Shop' })).toBeInTheDocument()
  expect(within(dialog).queryByRole('checkbox', { name: 'Blog' })).not.toBeInTheDocument()
})

it('403 account_not_in_scope is shown', async () => {
  mock([])
  server.use(
    http.post(`${API}/api/v1/target-groups`, () =>
      HttpResponse.json({ code: 'account_not_in_scope', status: 403, title: 'x' }, { status: 403 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(<TargetGroupsPage />)
  await user.click(await screen.findByRole('button', { name: 'New group' }))
  const dialog = await screen.findByRole('dialog')
  await user.type(within(dialog).getByLabelText('Name'), 'Spring')
  await user.click(within(dialog).getByRole('button', { name: 'Save group' }))
  expect(await within(dialog).findByText("That would affect accounts you don't have access to.")).toBeInTheDocument()
})
