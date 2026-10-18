import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Route, Routes } from 'react-router'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { setLocale } from '@/i18n'
import arJobs from '@/i18n/ar/jobs.json'
import { JobDetailPage } from './JobDetailPage'

const ID = '11111111-1111-7111-8111-111111111111'
const NEW_ID = '22222222-2222-7222-8222-222222222222'
const PREV = '33333333-3333-7333-8333-333333333333'

const job = (over: Record<string, unknown> = {}) => ({
  id: ID,
  job_type: 'mail.send',
  queue: 'mail',
  state: 'dead',
  attempts: 2,
  max_attempts: 2,
  created_at: '2026-10-08T10:00:00Z',
  run_at: '2026-10-08T10:00:00Z',
  finished_at: '2026-10-08T10:05:00Z',
  summary: { to: 'a@x.com' },
  actions: { retry: true, cancel: false, delete: true },
  ...over,
})

const attempts = [
  {
    attempt: 1,
    started_at: '2026-10-08T10:00:00Z',
    finished_at: '2026-10-08T10:00:02Z',
    worker: 'worker-a',
    outcome: 'failed',
    error_kind: 'transient',
    error_code: 'smtp_down',
    error_message: 'connection refused',
  },
  {
    attempt: 2,
    started_at: '2026-10-08T10:04:00Z',
    finished_at: '2026-10-08T10:04:01Z',
    worker: 'worker-b',
    outcome: 'failed',
    error_kind: 'permanent',
    error_code: 'rejected',
    error_message: null,
  },
]

function detail(over: Record<string, unknown> = {}, jobOver: Record<string, unknown> = {}) {
  server.use(
    http.get(`${API}/api/v1/admin/jobs/${ID}`, () =>
      HttpResponse.json({ job: job(jobOver), attempts, retried_from: PREV, retried_by: NEW_ID, ...over }),
    ),
  )
}

function view() {
  return renderApp(null, {
    route: `/jobs/${ID}`,
    routes: (
      <Routes>
        <Route path="/jobs/:id" element={<JobDetailPage />} />
        <Route path="/jobs/list" element={<h1>List page</h1>} />
      </Routes>
    ),
  })
}

beforeEach(() => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin' }))))
})

it('shows ids, retry chain links and every attempt', async () => {
  detail()
  view()
  expect(await screen.findByText(ID)).toBeInTheDocument()
  expect(screen.getByRole('link', { name: /retry of/i })).toHaveAttribute('href', `/jobs/${PREV}`)
  expect(screen.getByRole('link', { name: /retried as/i })).toHaveAttribute('href', `/jobs/${NEW_ID}`)
  expect(screen.getByText('worker-a')).toBeInTheDocument()
  expect(screen.getByText('worker-b')).toBeInTheDocument()
  expect(screen.getByText(/smtp_down/)).toBeInTheDocument()
  expect(screen.getByText(/rejected/)).toBeInTheDocument()
})

it('shows an error message only when the API provides one', async () => {
  detail()
  view()
  expect(await screen.findByText('connection refused')).toBeInTheDocument()
  expect(screen.getAllByText('connection refused')).toHaveLength(1)
  expect(screen.queryByText(/null/)).not.toBeInTheDocument()
})

it('renders only the allowed actions', async () => {
  detail()
  view()
  expect(await screen.findByRole('button', { name: 'Retry' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Cancel job' })).not.toBeInTheDocument()
})

it('explains why no action applies', async () => {
  detail({}, { actions: { retry: false, cancel: false, delete: false }, actions_disabled_reason: 'Unknown job type' })
  view()
  expect(await screen.findByText('Unknown job type')).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
})

it('retries after confirmation and opens the new job', async () => {
  detail()
  const spy = vi.fn()
  server.use(
    http.post(`${API}/api/v1/admin/jobs/${ID}/retry`, () => {
      spy()
      return HttpResponse.json({ job_id: NEW_ID })
    }),
    http.get(`${API}/api/v1/admin/jobs/${NEW_ID}`, () =>
      HttpResponse.json({ job: job({ id: NEW_ID, state: 'queued' }), attempts: [] }),
    ),
  )
  const user = userEvent.setup()
  view()
  await user.click(await screen.findByRole('button', { name: 'Retry' }))
  const dialog = await screen.findByRole('alertdialog')
  expect(within(dialog).getByText(/a new job with the same details is queued/i)).toBeInTheDocument()
  await user.click(within(dialog).getByRole('button', { name: 'Retry' }))
  expect(await screen.findByText(NEW_ID)).toBeInTheDocument()
  expect(spy).toHaveBeenCalledTimes(1)
})

it('maps a 409 to a friendly alert and stays on the page', async () => {
  detail()
  server.use(
    http.delete(`${API}/api/v1/admin/jobs/${ID}`, () =>
      HttpResponse.json({ code: 'job_action_not_allowed' }, { status: 409 }),
    ),
  )
  const user = userEvent.setup()
  view()
  await user.click(await screen.findByRole('button', { name: 'Delete' }))
  const dialog = await screen.findByRole('alertdialog')
  await user.click(within(dialog).getByRole('button', { name: 'Delete' }))
  expect(await screen.findByRole('alert')).toHaveTextContent(/can't take this action right now/i)
  expect(screen.getByText(ID)).toBeInTheDocument()
})

it('refetches the job after a 409 so the actions update', async () => {
  let gets = 0
  server.use(
    http.get(`${API}/api/v1/admin/jobs/${ID}`, () => {
      gets += 1
      const actions =
        gets === 1 ? { retry: true, cancel: false, delete: true } : { retry: true, cancel: false, delete: false }
      return HttpResponse.json({ job: job({ actions }), attempts })
    }),
    http.delete(`${API}/api/v1/admin/jobs/${ID}`, () =>
      HttpResponse.json({ code: 'job_action_not_allowed' }, { status: 409 }),
    ),
  )
  const user = userEvent.setup()
  view()
  await user.click(await screen.findByRole('button', { name: 'Delete' }))
  const dialog = await screen.findByRole('alertdialog')
  await user.click(within(dialog).getByRole('button', { name: 'Delete' }))
  await screen.findByRole('alert')
  await vi.waitFor(() => {
    expect(gets).toBe(2)
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument()
  })
})

describe('Arabic', () => {
  beforeEach(async () => {
    await setLocale('ar')
  })
  afterEach(async () => {
    await setLocale('en-US')
  })

  it('attempt payload stays LTR', async () => {
    detail()
    view()
    expect(await screen.findByRole('heading', { name: arJobs.detail.title })).toBeInTheDocument()
    expect(await screen.findByText('connection refused')).toHaveAttribute('dir', 'ltr')
    expect(screen.getByText(/smtp_down/)).toHaveAttribute('dir', 'ltr')
    expect(screen.getByText(ID)).toHaveAttribute('dir', 'ltr')
    expect(screen.getByText('worker-a')).toHaveAttribute('dir', 'ltr')
    // the attempt counter is a quantity: Arabic-Indic
    expect(screen.getAllByText('٢/٢').length).toBeGreaterThan(0)
  })
})
