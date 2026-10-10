import { screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { expect, it } from 'vitest'
import { Shell } from '@/components/Shell'
import { Gate, RequireAdmin, RequireSession } from '@/features/me/Gate'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'

const ID = '11111111-1111-7111-8111-111111111111'

function app() {
  return (
    <Routes>
      <Route element={<RequireSession />}>
        <Route element={<Gate />}>
          <Route element={<Shell />}>
            <Route index element={<div>dashboard</div>} />
            <Route element={<RequireAdmin />}>
              <Route path="jobs" element={<div>jobs dashboard</div>} />
              <Route path="jobs/list" element={<div>jobs list</div>} />
              <Route path="jobs/recurring" element={<div>jobs recurring</div>} />
              <Route path="jobs/:id" element={<div>job detail</div>} />
            </Route>
          </Route>
        </Route>
      </Route>
    </Routes>
  )
}

function as(role: 'admin' | 'member') {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ status: 'active', role }))),
    http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [], page: 1, page_size: 1, total: 0 })),
    http.get(`${API}/api/v1/admin/jobs/stats`, () =>
      HttpResponse.json({ states: [], queues: [], types: [], throughput: [], window_hours: 24 }),
    ),
  )
}

for (const path of ['/jobs', '/jobs/list', '/jobs/recurring', `/jobs/${ID}`]) {
  it(`forbids members on ${path}, as for /users and /audit`, async () => {
    as('member')
    renderApp(null, { routes: app(), route: path })
    expect(await screen.findByRole('heading', { name: /not allowed/i })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /jobs/i })).not.toBeInTheDocument()
  })

  it(`admits admins to ${path}`, async () => {
    as('admin')
    renderApp(null, { routes: app(), route: path })
    expect(await screen.findByText(/^(jobs dashboard|jobs list|jobs recurring|job detail)$/)).toBeInTheDocument()
  })
}
