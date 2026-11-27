import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap } from '@/api/client'
import type { components } from '@/api/schema'
import { useWsKey } from '@/workspace/keys'

export type CredentialCheck = components['schemas']['CredentialCheckResponse']
export type PendingView = components['schemas']['PendingConnectionDto']
export type Confirmed = components['schemas']['ConfirmResponse']

/** Checks pasted credentials against the platform. The answer carries the one-time `connect_id`. */
export function useStartCredentials(platform: string) {
  const api = useApi()
  return useMutation({
    mutationFn: (values: Record<string, unknown>) =>
      unwrap(
        api.POST('/api/v1/accounts/credentials/{platform}', {
          params: { path: { platform } },
          body: values as never,
        }),
      ),
  })
}

/** Starts an OAuth sign-in; the browser then goes to the platform's `authorize_url`. */
export function useStartOAuth() {
  const api = useApi()
  return useMutation({
    mutationFn: ({ platform, returnTo }: { platform: string; returnTo: string }) =>
      unwrap(
        api.POST('/api/v1/accounts/connect/{id}', {
          params: { path: { id: platform } },
          body: { return_to: returnTo },
        }),
      ),
  })
}

/** The accounts a started connection reached. A `404` is one that expired, was used, or is another user's. */
export function useConnectInfo(connectId: string | null) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'connect', connectId),
    queryFn: () => unwrap(api.GET('/api/v1/accounts/connect/{id}', { params: { path: { id: connectId ?? '' } } })),
    enabled: connectId !== null,
    retry: false,
    staleTime: Infinity,
  })
}

export function useConfirmConnection(connectId: string) {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: (accountIds: string[]) =>
      unwrap(
        api.POST('/api/v1/accounts/connect/{connect_id}/confirm', {
          params: { path: { connect_id: connectId } },
          body: { account_ids: accountIds },
        }),
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: key('accounts') }),
  })
}

/** Cancels an unfinished connection by its row. */
export function useDiscardConnection() {
  const api = useApi()
  return useMutation({
    mutationFn: (pendingId: string) =>
      ensureOk(api.DELETE('/api/v1/accounts/pending-connections/{id}', { params: { path: { id: pendingId } } })),
  })
}
