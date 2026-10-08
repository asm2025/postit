import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture, userFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { UsersPage } from './UsersPage'

const SELF_ID = '33333333-3333-7333-8333-333333333333'
const rows = [
  userFixture({
    id: '11111111-1111-7111-8111-111111111111',
    display_name: 'Pat Pending',
    email: 'pat@x.com',
    status: 'pending',
  }),
  userFixture({
    id: '22222222-2222-7222-8222-222222222222',
    display_name: 'Ann Active',
    email: 'ann@x.com',
    status: 'active',
  }),
  userFixture({ id: SELF_ID, display_name: 'Root Admin', email: 'root@x.com', status: 'active', role: 'admin' }),
  userFixture({
    id: '44444444-4444-7444-8444-444444444444',
    display_name: 'Dee Deleting',
    email: 'dee@x.com',
    status: 'deleting',
  }),
]

beforeEach(() => {
  server.use(
    http.get(`${API}/api/v1/me`, () =>
      HttpResponse.json(meFixture({ id: SELF_ID, role: 'admin', display_name: 'Root Admin' })),
    ),
  )
})

function list(spy = vi.fn()) {
  server.use(
    http.get(`${API}/api/v1/users`, ({ request }) => {
      spy(new URL(request.url).searchParams.toString())
      return HttpResponse.json({ data: rows, page: 1, page_size: 20, total: rows.length })
    }),
  )
  return spy
}

const rowOf = async (name: string) => (await screen.findByText(name)).closest('tr')!

async function openActions(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(within(await rowOf(name)).getByRole('button', { name: /actions/i }))
}

it('reads filters from the URL and sends them to the API', async () => {
  const spy = list()
  const user = userEvent.setup()
  renderApp(<UsersPage />, { route: '/users?status=pending&role=member' })
  expect(await screen.findByText('Pat Pending')).toBeInTheDocument()
  expect(spy).toHaveBeenCalledWith(expect.stringContaining('status=pending'))
  expect(spy).toHaveBeenCalledWith(expect.stringContaining('role=member'))
  await user.type(screen.getByRole('searchbox'), 'ann')
  await waitFor(() => {
    expect(spy).toHaveBeenLastCalledWith(expect.stringContaining('search=ann'))
  })
})

it('approves a pending user without a confirmation', async () => {
  list()
  const patched = vi.fn()
  server.use(
    http.patch(`${API}/api/v1/users/:id`, async ({ request, params }) => {
      patched(params.id, await request.json())
      return HttpResponse.json(rows[0])
    }),
  )
  const user = userEvent.setup()
  renderApp(<UsersPage />)
  await openActions(user, 'Pat Pending')
  await user.click(await screen.findByRole('menuitem', { name: /approve/i }))
  await waitFor(() => {
    expect(patched).toHaveBeenCalledWith(rows[0].id, { status: 'active' })
  })
})

it('confirms before deleting', async () => {
  list()
  const removed = vi.fn()
  server.use(
    http.delete(`${API}/api/v1/users/:id`, ({ params }) => {
      removed(params.id)
      return new HttpResponse(null, { status: 202 })
    }),
  )
  const user = userEvent.setup()
  renderApp(<UsersPage />)
  await openActions(user, 'Ann Active')
  await user.click(await screen.findByRole('menuitem', { name: /delete/i }))
  expect(removed).not.toHaveBeenCalled()
  await user.click(await screen.findByRole('button', { name: /^delete$/i }))
  await waitFor(() => {
    expect(removed).toHaveBeenCalledWith(rows[1].id)
  })
})

it('shows the last_admin refusal inline and keeps the row', async () => {
  list()
  server.use(
    http.patch(`${API}/api/v1/users/:id`, () =>
      HttpResponse.json({ code: 'last_admin', status: 409, title: 'x' }, { status: 409 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(<UsersPage />)
  await openActions(user, 'Ann Active')
  await user.click(await screen.findByRole('menuitem', { name: /disable/i }))
  await user.click(await screen.findByRole('button', { name: /^disable$/i }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/last active admin/i)
  expect(screen.getByText('Ann Active')).toBeInTheDocument()
})

it('never offers delete on your own row and warns before self-demotion', async () => {
  list()
  const patched = vi.fn()
  server.use(
    http.patch(`${API}/api/v1/users/:id`, async ({ request, params }) => {
      patched(params.id, await request.json())
      return HttpResponse.json(rows[2])
    }),
  )
  const user = userEvent.setup()
  renderApp(<UsersPage />)
  expect(within(await rowOf('Root Admin')).getByRole('link', { name: /root admin/i })).toHaveAttribute(
    'href',
    '/profile',
  )
  await openActions(user, 'Root Admin')
  expect(screen.queryByRole('menuitem', { name: /delete/i })).not.toBeInTheDocument()
  await user.click(await screen.findByRole('menuitem', { name: /make member/i }))
  expect(await screen.findByText(/lose admin access/i)).toBeInTheDocument()
  expect(patched).not.toHaveBeenCalled()
  await user.click(screen.getByRole('button', { name: /^make member$/i }))
  await waitFor(() => {
    expect(patched).toHaveBeenCalledWith(SELF_ID, { role: 'member' })
  })
})

it('offers no actions on a row that is being deleted', async () => {
  list()
  renderApp(<UsersPage />)
  const row = await rowOf('Dee Deleting')
  expect(within(row).queryByRole('button', { name: /actions/i })).not.toBeInTheDocument()
})
