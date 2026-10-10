import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap } from '@/api/client'
import type { components } from '@/api/schema'

export type Job = components['schemas']['JobDto']
export type JobDetail = components['schemas']['JobDetailDto']
export type JobStats = components['schemas']['JobStatsDto']
export type RecurringJob = components['schemas']['RecurringJobDto']
export type JobState = Job['state']

export interface JobFilters {
  state?: JobState
  jobType?: string
  from?: string
  to?: string
  page: number
  pageSize?: number
}

const DASHBOARD_POLL = 5_000
// The nav badge is on every admin page; poll it like the home panel's pending count, not
// like the dashboard, so an idle admin tab does not run the stats aggregates every 5 s.
const BADGE_POLL = 30_000
const visible = () => typeof document === 'undefined' || document.visibilityState !== 'hidden'

function query(f: JobFilters) {
  return {
    state: f.state,
    job_type: f.jobType,
    from: f.from,
    to: f.to,
    page: f.page,
    page_size: f.pageSize ?? 20,
  }
}

export function useJobs(filters: JobFilters, opts: { refetchInterval?: number } = {}) {
  const api = useApi()
  return useQuery({
    queryKey: ['jobs', 'list', filters],
    queryFn: () => unwrap(api.GET('/api/v1/admin/jobs', { params: { query: query(filters) } })),
    placeholderData: keepPreviousData,
    refetchInterval: opts.refetchInterval,
  })
}

export function useJob(id: string) {
  const api = useApi()
  return useQuery({
    queryKey: ['jobs', 'detail', id],
    queryFn: () => unwrap(api.GET('/api/v1/admin/jobs/{id}', { params: { path: { id } } })),
  })
}

export function useJobStats({
  enabled = true,
  interval = DASHBOARD_POLL,
}: { enabled?: boolean; interval?: number } = {}) {
  const api = useApi()
  return useQuery({
    // One cache entry: TanStack Query runs each observer's own interval, so the badge
    // polls at 30 s and the dashboard (when open) at 5 s against the same data.
    queryKey: ['jobs', 'stats'],
    queryFn: () => unwrap(api.GET('/api/v1/admin/jobs/stats', { params: { query: { window_hours: 24 } } })),
    enabled,
    refetchInterval: () => (visible() ? interval : false),
    refetchIntervalInBackground: false,
  })
}

/** Failed, dead and killed jobs combined (the sidebar badge). */
export function useFailedJobCount(enabled: boolean) {
  const stats = useJobStats({ enabled, interval: BADGE_POLL })
  const n = (key: string) => stats.data?.states.find((s) => s.key === key)?.count ?? 0
  return { ...stats, data: stats.data ? n('failed') + n('dead') + n('killed') : undefined }
}

export function useRecurringJobs() {
  const api = useApi()
  return useQuery({
    queryKey: ['jobs', 'recurring'],
    queryFn: () => unwrap(api.GET('/api/v1/admin/jobs/recurring')),
  })
}

export function useJobActions() {
  const api = useApi()
  const qc = useQueryClient()
  const done = () => {
    void qc.invalidateQueries({ queryKey: ['jobs'] })
    void qc.invalidateQueries({ queryKey: ['audit'] })
  }
  return {
    retry: useMutation({
      mutationFn: async (id: string) =>
        (await unwrap(api.POST('/api/v1/admin/jobs/{id}/retry', { params: { path: { id } } }))).job_id,
      onSettled: done,
    }),
    cancel: useMutation({
      mutationFn: (id: string) => ensureOk(api.POST('/api/v1/admin/jobs/{id}/cancel', { params: { path: { id } } })),
      onSettled: done,
    }),
    remove: useMutation({
      mutationFn: (id: string) => ensureOk(api.DELETE('/api/v1/admin/jobs/{id}', { params: { path: { id } } })),
      onSettled: done,
    }),
    trigger: useMutation({
      mutationFn: async (name: string) =>
        (await unwrap(api.POST('/api/v1/admin/jobs/recurring/{name}/trigger', { params: { path: { name } } }))).job_id,
      onSettled: done,
    }),
  }
}
