import { screen, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
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
  expect(await within(jobs).findByText('5')).toBeInTheDocument()
})

it('hides admin navigation from members', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByText('page body')).toBeInTheDocument()
  expect(screen.queryByRole('link', { name: /users/i })).not.toBeInTheDocument()
  expect(screen.queryByRole('link', { name: /jobs/i })).not.toBeInTheDocument()
})
