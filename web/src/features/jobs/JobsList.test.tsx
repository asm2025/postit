import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, expect, it, vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { JobsList } from './JobsList'

const ID = '11111111-1111-7111-8111-111111111111'

const jobFixture = (over: Record<string, unknown> = {}) => ({
  id: ID,
  job_type: 'mail.send',
  queue: 'mail',
  state: 'dead',
  attempts: 3,
  max_attempts: 3,
  created_at: '2026-10-08T10:00:00Z',
  run_at: '2026-10-08T10:00:00Z',
  finished_at: '2026-10-08T10:05:00Z',
  summary: { to: 'a@x.com' },
  actions: { retry: true, cancel: false, delete: true },
  ...over,
})

beforeEach(() => {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin' }))),
    http.get(`${API}/api/v1/admin/jobs/stats`, () =>
      HttpResponse.json({
        states: [],
        queues: [],
        types: [{ key: 'mail.send', count: 2 }],
        throughput: [],
        window_hours: 24,
      }),
    ),
  )
})

function list(rows: unknown[] = [jobFixture()], spy = vi.fn<(p: URLSearchParams) => void>()) {
  server.use(
    http.get(`${API}/api/v1/admin/jobs`, ({ request }) => {
      spy(new URL(request.url).searchParams)
      return HttpResponse.json({ data: rows, page: 1, page_size: 20, total: 45 })
    }),
  )
  return spy
}

it('renders a tab per state and marks the one from the URL', async () => {
  list()
  renderApp(<JobsList />, { route: '/jobs/list?state=dead' })
  expect(await screen.findByRole('tab', { name: 'All' })).toBeInTheDocument()
  expect(screen.getByRole('tab', { name: 'Queued' })).toBeInTheDocument()
  expect(screen.getByRole('tab', { name: 'Dead' })).toHaveAttribute('aria-selected', 'true')
})

it('requests the selected state', async () => {
  const spy = list()
  const user = userEvent.setup()
  renderApp(<JobsList />, { route: '/jobs/list' })
  await user.click(await screen.findByRole('tab', { name: 'Dead' }))
  await vi.waitFor(() => {
    expect(spy.mock.calls.some((c) => c[0].get('state') === 'dead')).toBe(true)
  })
})

it('sends from as RFC 3339', async () => {
  const spy = list()
  renderApp(<JobsList />, { route: '/jobs/list?from=2026-10-08T10:00' })
  await vi.waitFor(() => {
    expect(spy.mock.calls.at(-1)?.[0].get('from')).toBe(new Date('2026-10-08T10:00').toISOString())
  })
})

it('pages', async () => {
  const spy = list()
  const user = userEvent.setup()
  renderApp(<JobsList />, { route: '/jobs/list' })
  await user.click(await screen.findByRole('button', { name: /next/i }))
  await vi.waitFor(() => {
    expect(spy.mock.calls.at(-1)?.[0].get('page')).toBe('2')
  })
})

it('links rows to the detail page', async () => {
  list()
  renderApp(<JobsList />, { route: '/jobs/list' })
  const link = await screen.findByRole('link', { name: 'mail.send' })
  expect(link).toHaveAttribute('href', `/jobs/${ID}`)
  expect(screen.getByText('to: a@x.com')).toBeInTheDocument()
  expect(screen.getByText('3/3')).toBeInTheDocument()
})

it('shows an empty message', async () => {
  list([])
  renderApp(<JobsList />, { route: '/jobs/list' })
  expect(await screen.findByText('No jobs.')).toBeInTheDocument()
})
