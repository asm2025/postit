import { screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { setLocale } from '@/i18n'
import arJobs from '@/i18n/ar/jobs.json'
import { JobsDashboard } from './JobsDashboard'

const statsFixture = {
  states: [
    { key: 'dead', count: 1 },
    { key: 'queued', count: 4 },
  ],
  queues: [{ key: 'mail', count: 3 }],
  types: [],
  throughput: [
    { start: '2026-10-08T10:00:00Z', succeeded: 5, failed: 1 },
    { start: '2026-10-08T11:00:00Z', succeeded: 7, failed: 0 },
  ],
  window_hours: 24,
}

// The dashboard loads the chart module lazily, and transforming it (recharts) can take many
// seconds on a loaded machine: load it once here, under a hook timeout sized for that, so each
// test's lazy import resolves from the module cache and the tests time only the rendering.
beforeAll(async () => {
  await import('./ThroughputChart')
}, 60_000)

beforeEach(() => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin' }))))
})

function stats(body: Record<string, unknown>, status = 200) {
  server.use(http.get(`${API}/api/v1/admin/jobs/stats`, () => HttpResponse.json(body, { status })))
}

it('shows a counter per state and the queue depth', async () => {
  stats(statsFixture)
  renderApp(<JobsDashboard />, { route: '/jobs' })
  const dead = await screen.findByRole('link', { name: /dead/i })
  expect(dead).toHaveTextContent('1')
  expect(dead).toHaveAttribute('href', '/jobs/list?state=dead')
  expect(screen.getByText(/mail/i)).toBeInTheDocument()
})

it('renders the throughput chart with a text alternative', async () => {
  stats(statsFixture)
  renderApp(<JobsDashboard />, { route: '/jobs' })
  // Rendering behind Suspense still takes longer than the default 1 s on a busy machine.
  expect(await screen.findByRole('img', { name: /jobs completed per hour/i }, { timeout: 5000 })).toBeInTheDocument()
  expect(screen.getByRole('table', { name: /throughput data/i })).toBeInTheDocument()
})

it('shows an error message when stats fail to load', async () => {
  stats({ code: 'internal' }, 500)
  renderApp(<JobsDashboard />, { route: '/jobs' })
  expect(await screen.findByRole('alert')).toBeInTheDocument()
})

describe('Arabic', () => {
  beforeEach(async () => {
    await setLocale('ar')
  })
  afterEach(async () => {
    vi.restoreAllMocks()
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe = vi.fn()
        unobserve = vi.fn()
        disconnect = vi.fn()
      },
    )
    await setLocale('en-US')
  })

  it('keeps the count axis ticks left-to-right under the right-to-left page', async () => {
    // jsdom has no layout: a stub observer reports a size so recharts draws the axes.
    class SizedObserver {
      private readonly cb: ResizeObserverCallback
      constructor(cb: ResizeObserverCallback) {
        this.cb = cb
      }
      observe(target: Element) {
        this.cb([{ target, contentRect: { width: 600, height: 224 } } as ResizeObserverEntry], this)
      }
      unobserve = vi.fn()
      disconnect = vi.fn()
    }
    vi.stubGlobal('ResizeObserver', SizedObserver)
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 600, 224))
    stats(statsFixture)
    const { container } = renderApp(<JobsDashboard />, { route: '/jobs' })
    await screen.findByRole('img', { name: arJobs.chart.label }, { timeout: 5000 })
    const ticks = container.querySelectorAll('.recharts-yAxis-tick-labels text')
    expect(ticks.length).toBeGreaterThan(0)
    for (const tick of ticks) expect(tick).toHaveAttribute('direction', 'ltr')
    const xTicks = container.querySelectorAll('.recharts-xAxis-tick-labels text')
    expect(xTicks.length).toBeGreaterThan(0)
    for (const tick of xTicks) expect(tick).not.toHaveAttribute('direction', 'ltr')
  })

  it('renders in Arabic', async () => {
    stats(statsFixture)
    renderApp(<JobsDashboard />, { route: '/jobs' })
    expect(await screen.findByRole('heading', { name: arJobs.title })).toBeInTheDocument()
    const dead = await screen.findByRole('link', { name: new RegExp(arJobs.states.dead) })
    // 1 dead job: Arabic-Indic digit
    expect(dead).toHaveTextContent('١')
    expect(await screen.findByRole('link', { name: new RegExp(arJobs.states.queued) })).toHaveTextContent('٤')
    expect(screen.getByText(arJobs.dashboard.states)).toBeInTheDocument()
    expect(screen.queryByText('Dead')).not.toBeInTheDocument()
    // queue names are identifiers: Western, isolated
    expect(screen.getByText('mail').closest('[dir]')).toHaveAttribute('dir', 'ltr')
    expect(screen.getByText('٣')).toBeInTheDocument()
  })
})
