import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { setLocale } from '@/i18n'
import arJobs from '@/i18n/ar/jobs.json'
import { RecurringPage } from './RecurringPage'

const NEW_ID = '22222222-2222-7222-8222-222222222222'

const rows = [
  {
    name: 'maintenance.sweep',
    schedule: '0 */5 * * * *',
    last_run_at: '2026-10-08T10:00:00Z',
    last_outcome: 'succeeded',
    next_run_at: '2026-10-08T10:05:00Z',
  },
  { name: 'mail.digest', schedule: '0 0 8 * * *', last_run_at: null, last_outcome: null, next_run_at: null },
]

beforeEach(() => {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin' }))),
    http.get(`${API}/api/v1/admin/jobs/recurring`, () => HttpResponse.json(rows)),
  )
})

function view() {
  return renderApp(null, {
    route: '/jobs/recurring',
    routes: (
      <Routes>
        <Route path="/jobs/recurring" element={<RecurringPage />} />
      </Routes>
    ),
  })
}

it('lists name, schedule, last run with outcome and next run', async () => {
  view()
  const row = (await screen.findByText('maintenance.sweep')).closest('tr')!
  expect(within(row).getByText('0 */5 * * * *')).toBeInTheDocument()
  expect(within(row).getByText('Succeeded')).toBeInTheDocument()
  const never = screen.getByText('mail.digest').closest('tr')!
  expect(within(never).getAllByText('—').length).toBeGreaterThanOrEqual(2)
})

it('triggers a job after confirmation and links to it', async () => {
  const posted = vi.fn()
  server.use(
    http.post(`${API}/api/v1/admin/jobs/recurring/maintenance.sweep/trigger`, () => {
      posted()
      return HttpResponse.json({ job_id: NEW_ID }, { status: 202 })
    }),
  )
  view()
  await userEvent.click(await screen.findByRole('button', { name: 'Trigger maintenance.sweep now' }))
  expect(posted).not.toHaveBeenCalled()
  const dialog = await screen.findByRole('alertdialog')
  expect(dialog).toHaveTextContent(/Run .*maintenance\.sweep.* now\? It runs once in the background\./)
  await userEvent.click(within(dialog).getByRole('button', { name: 'Run now' }))
  const status = await screen.findByRole('status')
  expect(status).toHaveTextContent('Triggered')
  expect(within(status).getByRole('link', { name: 'View job' })).toHaveAttribute('href', `/jobs/${NEW_ID}`)
  expect(posted).toHaveBeenCalledTimes(1)
})

it('shows an alert when the trigger fails with 404', async () => {
  server.use(
    http.post(`${API}/api/v1/admin/jobs/recurring/mail.digest/trigger`, () =>
      HttpResponse.json({ code: 'not_found', status: 404, title: 'Not found' }, { status: 404 }),
    ),
  )
  view()
  await userEvent.click(await screen.findByRole('button', { name: 'Trigger mail.digest now' }))
  await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Run now' }))
  expect(await screen.findByRole('alert')).toBeInTheDocument()
  expect(screen.queryByRole('status')).not.toBeInTheDocument()
})

describe('Arabic', () => {
  beforeEach(async () => {
    await setLocale('ar')
  })
  afterEach(async () => {
    await setLocale('en-US')
  })

  it('cron expression stays Western and isolated', async () => {
    view()
    expect(await screen.findByRole('heading', { name: arJobs.recurring.title })).toBeInTheDocument()
    const cron = await screen.findByText('0 */5 * * * *')
    expect(cron).toHaveAttribute('dir', 'ltr')
    expect(screen.getByText('maintenance.sweep')).toHaveAttribute('dir', 'ltr')
    expect(
      screen.getByRole('button', { name: arJobs.recurring.triggerAria.replace('{{name}}', 'maintenance.sweep') }),
    ).toBeInTheDocument()
    expect(screen.getAllByText(arJobs.recurring.triggerNow)).toHaveLength(2)
    expect(screen.queryByText('Trigger now')).not.toBeInTheDocument()
  })
})
