import { screen, waitFor, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { Route, Routes, useLocation } from 'react-router'
import { API, meFixture, notificationFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { Shell } from '@/components/Shell'
import { NotificationsPage } from './NotificationsPage'

function Where() {
  const { pathname, search } = useLocation()
  return (
    <p data-testid="where">
      {pathname}
      {search}
    </p>
  )
}

it('lists notifications with their read state and pages from the server', async () => {
  const seen = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/notifications`, ({ request }) => {
      seen(new URL(request.url).searchParams.toString())
      return HttpResponse.json({
        data: [
          notificationFixture('account_needs_reconnect', { id: 'n1' }),
          notificationFixture('delivery_failed', { id: 'n2', read_at: '2026-11-21T10:00:00Z', severity: 'critical' }),
        ],
        page: 1,
        page_size: 20,
        total: 2,
      })
    }),
  )
  renderApp(<NotificationsPage />)

  const table = await screen.findByRole('table', { name: 'Notifications' })
  const rows = within(table).getAllByRole('row').slice(1)
  expect(rows).toHaveLength(2)
  expect(within(rows[0]).getByText('Reconnect an account')).toBeInTheDocument()
  expect(within(rows[0]).getByText('Unread')).toBeInTheDocument()
  expect(within(rows[0]).getByRole('button', { name: 'Open' })).toBeInTheDocument()
  expect(within(rows[1]).getByText('Read')).toBeInTheDocument()
  expect(within(rows[1]).getByText('Needs attention')).toBeInTheDocument()
  // Nowhere to go yet: no Open button.
  expect(within(rows[1]).queryByRole('button', { name: 'Open' })).not.toBeInTheDocument()
  expect(seen).toHaveBeenCalledWith('page=1&page_size=20')
})

it('says so when there is nothing', async () => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())))
  renderApp(<NotificationsPage />)
  expect(await screen.findByText('No notifications yet.')).toBeInTheDocument()
})

describe('a push click', () => {
  const ID = '018f0000-0000-7000-8000-0000000000aa'
  const shell = (
    <Routes>
      <Route element={<Shell />}>
        <Route path="*" element={<Where />} />
      </Route>
    </Routes>
  )

  it('push click ?notification=id loads and opens it', async () => {
    const read = vi.fn()
    server.use(
      http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
      http.get(`${API}/api/v1/notifications/${ID}`, () =>
        HttpResponse.json(
          notificationFixture('delegation_changed', { id: ID, owner_id: '00000000-0000-7000-8000-000000000001' }),
        ),
      ),
      http.post(`${API}/api/v1/notifications/read`, async ({ request }) => {
        read(await request.json())
        return HttpResponse.json({ updated: 1 })
      }),
    )
    renderApp(null, { routes: shell, route: `/?notification=${ID}` })
    await waitFor(() => {
      expect(screen.getByTestId('where')).toHaveTextContent('/delegations?tab=granted')
    })
    expect(screen.getByTestId('where')).not.toHaveTextContent('notification=')
    expect(read).toHaveBeenCalledWith({ ids: [ID] })
  })

  it('push click for an unknown id lands on the list', async () => {
    server.use(
      http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
      http.get(`${API}/api/v1/notifications/${ID}`, () =>
        HttpResponse.json({ code: 'not_found', status: 404, title: 'Not found' }, { status: 404 }),
      ),
    )
    renderApp(null, { routes: shell, route: `/?notification=${ID}&keep=1` })
    await waitFor(() => {
      expect(screen.getByTestId('where')).toHaveTextContent(/^\/notifications$/)
    })
  })
})
