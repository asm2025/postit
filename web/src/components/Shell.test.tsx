import { screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { setLocale } from '@/i18n'
import arCommon from '@/i18n/ar/common.json'
import enCommon from '@/i18n/en-US/common.json'
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

it('renders in Arabic without English chrome', async () => {
  mock('admin')
  await setLocale('ar')
  try {
    renderApp(null, { routes: app() })
    expect(await screen.findByRole('link', { name: new RegExp(arCommon.shell.users) })).toBeInTheDocument()
    for (const english of Object.values(enCommon.shell)) {
      expect(screen.queryByText(english), `text ${english}`).not.toBeInTheDocument()
      expect(screen.queryByLabelText(english), `label ${english}`).not.toBeInTheDocument()
    }
    for (const english of Object.values(enCommon.theme)) {
      expect(screen.queryByLabelText(english), `label ${english}`).not.toBeInTheDocument()
    }
  } finally {
    await setLocale('en-US')
  }
})
