import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture, userFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { HomePage } from './HomePage'

const pat = userFixture({
  id: '11111111-1111-7111-8111-111111111111',
  display_name: 'Pat Pending',
  email: 'pat@x.com',
  status: 'pending',
})

interface Mock {
  adminTotal?: number
  audit?: 'ok' | 'fail'
}

function mock({ adminTotal = 2, audit = 'ok' }: Mock = {}) {
  const pending = [pat]
  server.use(
    http.get(`${API}/api/v1/users`, ({ request }) => {
      const p = new URL(request.url).searchParams
      if (p.get('role') === 'admin') return HttpResponse.json({ data: [], page: 1, page_size: 1, total: adminTotal })
      const status = p.get('status')
      const total = status === 'pending' ? pending.length : status === 'active' ? 5 : 1
      // The panel must ask for the longest-waiting users first.
      const data = status === 'pending' && p.get('sort') === 'created_at' ? pending : []
      return HttpResponse.json({ data, page: 1, page_size: Number(p.get('page_size')), total })
    }),
    http.get(`${API}/api/v1/admin/audit`, () =>
      audit === 'fail'
        ? HttpResponse.json({ code: 'internal', status: 500, title: 'x' }, { status: 500 })
        : HttpResponse.json({
            data: [
              {
                id: 'e1',
                at: '2026-10-05T11:00:00Z',
                kind: 'user_provisioned',
                actor: null,
                subject: { id: 's', deleted: false },
                owner: null,
                ip: null,
                request_id: null,
                details: {},
              },
            ],
            page: 1,
            page_size: 8,
            total: 1,
          }),
    ),
  )
}

const asAdmin = () => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin', display_name: 'Root' }))))
}
const asMember = () => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'member', display_name: 'Mia' }))))
}

it('shows only the account card to members and never calls admin endpoints', async () => {
  asMember()
  const adminCall = vi.fn()
  server.use(
    http.get(`${API}/api/v1/users`, () => {
      adminCall()
      return HttpResponse.json({})
    }),
    http.get(`${API}/api/v1/admin/audit`, () => {
      adminCall()
      return HttpResponse.json({})
    }),
  )
  renderApp(<HomePage />)
  expect(await screen.findByText('Mia')).toBeInTheDocument()
  expect(screen.queryByText(/needs approval/i)).not.toBeInTheDocument()
  expect(adminCall).not.toHaveBeenCalled()
})

it('shows admin panels: pending users, counters and recent activity', async () => {
  asAdmin()
  mock()
  renderApp(<HomePage />)
  expect(await screen.findByText('Pat Pending')).toBeInTheDocument()
  expect(await screen.findByText('User signed up')).toBeInTheDocument()
  expect(screen.getByRole('link', { name: /active/i })).toHaveAttribute('href', '/users?status=active')
})

it('approves from Home', async () => {
  asAdmin()
  mock()
  const patched = vi.fn()
  server.use(
    http.patch(`${API}/api/v1/users/:id`, async ({ request, params }) => {
      patched(params.id, await request.json())
      return HttpResponse.json(pat)
    }),
  )
  const user = userEvent.setup()
  renderApp(<HomePage />)
  const row = (await screen.findByText('Pat Pending')).closest('li')!
  await user.click(within(row).getByRole('button', { name: /approve/i }))
  await waitFor(() => {
    expect(patched).toHaveBeenCalledWith(pat.id, { status: 'active' })
  })
})

it('asks for confirmation before rejecting', async () => {
  asAdmin()
  mock()
  const removed = vi.fn()
  server.use(
    http.delete(`${API}/api/v1/users/:id`, ({ params }) => {
      removed(params.id)
      return new HttpResponse(null, { status: 202 })
    }),
  )
  const user = userEvent.setup()
  renderApp(<HomePage />)
  const row = (await screen.findByText('Pat Pending')).closest('li')!
  await user.click(within(row).getByRole('button', { name: /reject/i }))
  expect(removed).not.toHaveBeenCalled()
  await user.click(await screen.findByRole('button', { name: /^reject$/i }))
  await waitFor(() => {
    expect(removed).toHaveBeenCalledWith(pat.id)
  })
})

it('warns when exactly one active admin exists', async () => {
  asAdmin()
  mock({ adminTotal: 1 })
  renderApp(<HomePage />)
  expect(await screen.findByText(/only one active admin/i)).toBeInTheDocument()
})

it('does not warn with two active admins', async () => {
  asAdmin()
  mock({ adminTotal: 2 })
  renderApp(<HomePage />)
  await screen.findByText('Pat Pending')
  expect(screen.queryByText(/only one active admin/i)).not.toBeInTheDocument()
})

it('isolates a failing panel', async () => {
  asAdmin()
  mock({ audit: 'fail' })
  renderApp(<HomePage />)
  expect(await screen.findByText('Pat Pending')).toBeInTheDocument()
  expect(await screen.findByRole('alert')).toHaveTextContent(/went wrong on the server/i)
})
