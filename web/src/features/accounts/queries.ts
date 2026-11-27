import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'
import { useWsKey } from '@/workspace/keys'

export type Account = components['schemas']['AccountDto']

export interface AccountFilters {
  platform?: string
  includeDisconnected?: boolean
  page: number
  pageSize: number
}

/** The accounts of the active workspace the caller reaches, newest first. */
export function useAccounts(filters: AccountFilters) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'list', filters),
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/accounts', {
          params: {
            query: {
              platform: filters.platform,
              include_disconnected: filters.includeDisconnected,
              page: filters.page,
              page_size: filters.pageSize,
            },
          },
        }),
      ),
    placeholderData: keepPreviousData,
  })
}
