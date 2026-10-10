import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { ApiContext } from '@/api/ApiContext'
import { createApiClient } from '@/api/client'
import { server } from '@/test/server'
import { API } from '@/test/fixtures'
import { useFailedJobCount, useJob, useJobActions, useJobStats } from './queries'

const stats = {
  states: [
    { key: 'failed', count: 2 },
    { key: 'dead', count: 1 },
    { key: 'killed', count: 3 },
    { key: 'queued', count: 9 },
  ],
  queues: [],
  types: [],
  throughput: [],
  window_hours: 24,
}

function wrapper() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const api = createApiClient(API, {
    getAccessToken: () => 'test-token',
    renew: () => Promise.resolve(null),
    signOut: () => undefined,
  })
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ApiContext.Provider value={api}>{children}</ApiContext.Provider>
    </QueryClientProvider>
  )
  return { Wrapper, queryClient }
}

function countStatsRequests() {
  const spy = vi.fn()
  server.use(
    http.get(`${API}/api/v1/admin/jobs/stats`, () => {
      spy()
      return HttpResponse.json(stats)
    }),
  )
  return spy
}

function visibility(state: 'visible' | 'hidden') {
  vi.spyOn(document, 'visibilityState', 'get').mockReturnValue(state)
}

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

it('sums failed, dead and killed for the badge', async () => {
  countStatsRequests()
  const { Wrapper } = wrapper()
  const { result } = renderHook(() => useFailedJobCount(true), { wrapper: Wrapper })
  await waitFor(() => {
    expect(result.current.data).toBe(6)
  })
})

describe('polling', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })

  it('pauses while the tab is hidden and resumes when visible', async () => {
    const spy = countStatsRequests()
    const { Wrapper } = wrapper()
    renderHook(() => useJobStats(), { wrapper: Wrapper })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(1)
    })

    visibility('hidden')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_000)
    })
    expect(spy).toHaveBeenCalledTimes(1)

    visibility('visible')
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000)
    })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(2)
    })
  })

  it.each([
    ['running', 2],
    ['succeeded', 1],
  ])('a %s job page polls until the job settles (%i requests in 5 s)', async (state, calls) => {
    const spy = vi.fn()
    server.use(
      http.get(`${API}/api/v1/admin/jobs/:id`, () => {
        spy()
        return HttpResponse.json({ job: { state }, attempts: [], retried_from: null, retried_by: null })
      }),
    )
    const { Wrapper } = wrapper()
    renderHook(() => useJob('job-1'), { wrapper: Wrapper })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(1)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000)
    })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(calls)
    })
  })

  it('polls the badge every 30 s, not every 5 s', async () => {
    const spy = countStatsRequests()
    const { Wrapper } = wrapper()
    renderHook(() => useFailedJobCount(true), { wrapper: Wrapper })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(1)
    })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000)
    })
    expect(spy).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000)
    })
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(2)
    })
  })
})

it('retry posts to the job and resolves to the new id, invalidating the jobs cache', async () => {
  const newId = '99999999-9999-7999-8999-999999999999'
  const spy = vi.fn()
  server.use(
    http.post(`${API}/api/v1/admin/jobs/:id/retry`, ({ params }) => {
      spy(params.id)
      return HttpResponse.json({ job_id: newId })
    }),
  )
  const { Wrapper, queryClient } = wrapper()
  const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
  const { result } = renderHook(() => useJobActions(), { wrapper: Wrapper })

  let id: string | undefined
  await act(async () => {
    id = await result.current.retry.mutateAsync('job-1')
  })

  expect(spy).toHaveBeenCalledWith('job-1')
  expect(id).toBe(newId)
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['jobs'] })
})
