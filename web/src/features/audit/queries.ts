import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'
import type { AuditKind } from './kinds'

export type AuditEvent = components['schemas']['Page_AuditEventDto']['data'][number]
export type UserRef = components['schemas']['UserRef']

export interface AuditFilters {
  kind?: AuditKind
  from?: string
  to?: string
  actor?: string
  subject?: string
  page: number
  pageSize?: number
}

export function useAudit(f: AuditFilters, opts: { refetchInterval?: number } = {}) {
  const api = useApi()
  return useQuery({
    queryKey: ['audit', f],
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/admin/audit', {
          params: {
            query: {
              kind: f.kind,
              from: f.from ?? undefined,
              to: f.to ?? undefined,
              actor_user_id: f.actor ?? undefined,
              subject_user_id: f.subject ?? undefined,
              page: f.page,
              page_size: f.pageSize ?? 20,
            },
          },
        }),
      ),
    placeholderData: keepPreviousData,
    refetchInterval: opts.refetchInterval,
  })
}

export function useRecentAudit(limit: number) {
  return useAudit({ page: 1, pageSize: limit }, { refetchInterval: 30_000 })
}
