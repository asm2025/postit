import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { server } from '@/test/server'
import { API, delegatedWorkspaceFixture, meFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { Shell } from './Shell'

function app() {
  return (
    <Routes>
      <Route element={<Shell />}>
        <Route index element={<div>page body</div>} />
      </Route>
    </Routes>
  )
}

function mock(role: 'admin' | 'member') {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role }))),
    http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [], page: 1, page_size: 1, total: 3 })),
    http.get(`${API}/api/v1/admin/jobs/stats`, () =>
      HttpResponse.json({
        states: [
          { key: 'failed', count: 4 },
          { key: 'dead', count: 1 },
          { key: 'queued', count: 9 },
        ],
        queues: [],
        types: [],
        throughput: [],
        window_hours: 24,
      }),
    ),
  )
}

it('shows admin navigation and the pending badge to admins', async () => {
  mock('admin')
  renderApp(null, { routes: app() })
  expect(await screen.findByRole('link', { name: /users/i })).toBeInTheDocument()
  expect(await screen.findByText('3')).toBeInTheDocument()
  expect(screen.getByRole('link', { name: /audit/i })).toBeInTheDocument()
  const jobs = screen.getByRole('link', { name: /jobs/i })
  expect(jobs).toHaveAttribute('href', '/jobs')
  // The count is announced with its meaning, not as a bare number.
  expect(await screen.findByRole('link', { name: /jobs\s*5\s*failed jobs/i })).toBeInTheDocument()
})

it('hides admin navigation from members', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByText('page body')).toBeInTheDocument()
  expect(screen.queryByRole('link', { name: /users/i })).not.toBeInTheDocument()
  expect(screen.queryByRole('link', { name: /jobs/i })).not.toBeInTheDocument()
})

const OLIVE = '0000000a-0000-7000-8000-00000000000a'
const LIMITED = '0000000b-0000-7000-8000-00000000000b'
const workspaces = [
  ownWorkspaceFixture(),
  delegatedWorkspaceFixture(OLIVE, { level: 'edit' }),
  delegatedWorkspaceFixture(LIMITED, {
    level: 'publish',
    all_accounts: false,
    account_ids: ['a1', 'a2'],
    owner: { id: LIMITED, display_name: 'Lena Limited', status: 'active' },
  }),
]

it('switcher lists own and received workspaces', async () => {
  mock('member')
  renderApp(null, { routes: app(), workspaces })
  await userEvent.click(await screen.findByRole('combobox', { name: 'Workspace' }))
  const list = await screen.findByRole('listbox')
  expect(within(list).getByRole('option', { name: 'My workspace' })).toBeInTheDocument()
  expect(within(list).getByRole('option', { name: "Olive Owner's workspace · Can edit" })).toBeInTheDocument()
  expect(
    within(list).getByRole('option', { name: "Lena Limited's workspace · Can publish · 2 accounts" }),
  ).toBeInTheDocument()
})

it('switcher shows only whose workspace it is, with the full label as tooltip', async () => {
  mock('member')
  renderApp(null, { routes: app(), actAs: LIMITED, workspaces })
  const trigger = await screen.findByRole('combobox', { name: 'Workspace' })
  const full = "Lena Limited's workspace · Can publish · 2 accounts"
  expect(trigger).toHaveAttribute('title', full)
  expect(trigger).toHaveTextContent(/^Lena Limited's workspace$/)
  // Fills the sidebar column and ends in an ellipsis instead of growing past it.
  expect(trigger).toHaveClass('w-full', 'min-w-0', '*:data-[slot=select-value]:truncate')
  expect(trigger).not.toHaveClass('w-fit')

  await userEvent.click(trigger)
  const option = within(await screen.findByRole('listbox')).getByRole('option', { name: full })
  expect(option).toHaveClass('whitespace-normal', 'break-words')
})

it('has no switcher when the user has only their own workspace', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByText('page body')).toBeInTheDocument()
  expect(screen.queryByRole('combobox', { name: 'Workspace' })).not.toBeInTheDocument()
})

it('banner shows while delegated and its button returns to own', async () => {
  mock('member')
  renderApp(null, { routes: app(), actAs: OLIVE, workspaces })
  const banner = (await screen.findByText(/You are working in/)).closest<HTMLElement>('[role="status"]')
  if (!banner) throw new Error('no banner')
  expect(banner).toHaveTextContent("You are working in Olive Owner's workspace (Can edit)")

  await userEvent.click(within(banner).getByRole('button', { name: 'Back to my workspace' }))

  await waitFor(() => {
    expect(screen.queryByText(/You are working in/)).not.toBeInTheDocument()
  })
})

it('shows no banner in the own workspace', async () => {
  mock('member')
  renderApp(null, { routes: app(), workspaces })
  expect(await screen.findByText('page body')).toBeInTheDocument()
  expect(screen.queryByText(/You are working in/)).not.toBeInTheDocument()
})

it('Delegations shows in every workspace', async () => {
  mock('member')
  renderApp(null, { routes: app(), workspaces })
  expect(await screen.findByRole('link', { name: 'Delegations' })).toHaveAttribute('href', '/delegations')
})

it('Delegations shows while delegated too', async () => {
  mock('member')
  renderApp(null, { routes: app(), actAs: OLIVE, workspaces })
  expect(await screen.findByRole('link', { name: 'Delegations' })).toBeInTheDocument()
})

it('admin section only for admins', async () => {
  mock('admin')
  renderApp(null, { routes: app() })
  expect(await screen.findByRole('link', { name: 'Platforms' })).toHaveAttribute('href', '/admin/platforms')
  expect(screen.getByRole('link', { name: 'Publishing' })).toHaveAttribute('href', '/admin/publishing')
})

it('members see no admin links', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByText('page body')).toBeInTheDocument()
  expect(screen.queryByRole('link', { name: 'Platforms' })).not.toBeInTheDocument()
  expect(screen.queryByRole('link', { name: 'Publishing' })).not.toBeInTheDocument()
})

describe('in a right-to-left language', () => {
  beforeEach(() => {
    document.documentElement.dir = 'rtl'
  })
  afterEach(() => {
    document.documentElement.dir = 'ltr'
  })

  it('keeps the switcher and banner on logical edges', async () => {
    mock('member')
    const { container } = renderApp(null, { routes: app(), actAs: OLIVE, workspaces })
    await screen.findByRole('combobox', { name: 'Workspace' })
    await screen.findByText(/You are working in/)
    expect(container.querySelector('[class*="ml-"], [class*="mr-"], [class~="left-0"], [class~="right-0"]')).toBeNull()
    expect(container.querySelector('[class*="ms-"], [class*="me-"], [class*="ps-"], [class*="pe-"]')).not.toBeNull()
  })
})
