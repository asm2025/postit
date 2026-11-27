import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap } from '@/api/client'
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

/** One account of the active workspace. A `404` is an account that is not there or outside the grant's limit. */
export function useAccount(id: string) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'one', id),
    queryFn: () => unwrap(api.GET('/api/v1/accounts/{id}', { params: { path: { id } } })),
    retry: false,
  })
}

/** The owner's actions on an account. The routes are owner-only: a delegate never gets to call them. */
export function useAccountActions(id: string) {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  const refresh = () => qc.invalidateQueries({ queryKey: key('accounts') })
  const patch = (body: { display_name?: string; test_account?: boolean }) =>
    unwrap(api.PATCH('/api/v1/accounts/{id}', { params: { path: { id } }, body }))
  return {
    rename: useMutation({ mutationFn: (display_name: string) => patch({ display_name }), onSuccess: refresh }),
    setTestAccount: useMutation({ mutationFn: (test_account: boolean) => patch({ test_account }), onSuccess: refresh }),
    check: useMutation({
      mutationFn: () => unwrap(api.POST('/api/v1/accounts/{id}/check', { params: { path: { id } } })),
      onSuccess: refresh,
    }),
    disconnect: useMutation({
      mutationFn: () => unwrap(api.DELETE('/api/v1/accounts/{id}', { params: { path: { id } } })),
      onSuccess: refresh,
    }),
  }
}

export type WritingStyle = components['schemas']['WritingStyleDto']
export type WritingStyleInput = components['schemas']['PutWritingStyleRequest']

export function useWritingStyle(id: string) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'style', id),
    queryFn: () => unwrap(api.GET('/api/v1/accounts/{id}/writing-style', { params: { path: { id } } })),
  })
}

export function useSaveWritingStyle(id: string) {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: (body: WritingStyleInput) =>
      unwrap(api.PUT('/api/v1/accounts/{id}/writing-style', { params: { path: { id } }, body })),
    onSuccess: (saved) => {
      qc.setQueryData(key('accounts', 'style', id), saved)
    },
  })
}

export type PendingConnection = components['schemas']['PendingSummaryDto']

/** Connections the owner started and did not finish. Owner only: pass `enabled` false elsewhere. */
export function usePendingConnections(enabled: boolean) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'pending'),
    queryFn: () => unwrap(api.GET('/api/v1/accounts/pending-connections')),
    enabled,
  })
}

export function useCancelPending() {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: (pendingId: string) =>
      ensureOk(api.DELETE('/api/v1/accounts/pending-connections/{id}', { params: { path: { id: pendingId } } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: key('accounts', 'pending') }),
  })
}
