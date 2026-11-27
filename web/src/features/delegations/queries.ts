import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap } from '@/api/client'
import type { components } from '@/api/schema'
import { useWsKey } from '@/workspace/keys'

export type Delegation = components['schemas']['DelegationDto']
export type GrantRequest = components['schemas']['GrantDelegationRequest']
export type ChangeRequest = components['schemas']['ChangeDelegationRequest']

/** The grants the owner gave others. Own workspace only. */
export function useGranted(page: number, pageSize: number, enabled = true) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('delegations', 'granted', page, pageSize),
    queryFn: () => unwrap(api.GET('/api/v1/delegations', { params: { query: { page, page_size: pageSize } } })),
    placeholderData: keepPreviousData,
    enabled,
  })
}

/** The grants the caller holds in other people's workspaces. */
export function useReceived(page: number, pageSize: number) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('delegations', 'received', page, pageSize),
    queryFn: () =>
      unwrap(api.GET('/api/v1/delegations/received', { params: { query: { page, page_size: pageSize } } })),
    placeholderData: keepPreviousData,
  })
}

/** An exact-email lookup of an `active` user to grant access to. `null` means nobody has that email. */
export function useUserLookup(email: string) {
  const api = useApi()
  return useQuery({
    queryKey: ['users', 'lookup', email],
    queryFn: async () => (await unwrap(api.GET('/api/v1/users/lookup', { params: { query: { email } } }))).user ?? null,
    enabled: email !== '',
    retry: false,
    staleTime: 30_000,
  })
}

/** Every change to a grant also changes the workspaces the receiver can switch to. */
function useRefresh() {
  const qc = useQueryClient()
  const key = useWsKey()
  return () => {
    void qc.invalidateQueries({ queryKey: key('delegations') })
    void qc.invalidateQueries({ queryKey: ['workspaces'] })
  }
}

export function useGrant() {
  const api = useApi()
  const refresh = useRefresh()
  return useMutation({
    mutationFn: (body: GrantRequest) => unwrap(api.POST('/api/v1/delegations', { body })),
    onSuccess: refresh,
  })
}

export function useChangeGrant() {
  const api = useApi()
  const refresh = useRefresh()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: ChangeRequest }) =>
      unwrap(api.PATCH('/api/v1/delegations/{id}', { params: { path: { id } }, body })),
    onSuccess: refresh,
  })
}

export function useRevoke() {
  const api = useApi()
  const refresh = useRefresh()
  return useMutation({
    mutationFn: (id: string) => ensureOk(api.DELETE('/api/v1/delegations/{id}', { params: { path: { id } } })),
    onSuccess: refresh,
  })
}

export function useLeave() {
  const api = useApi()
  const refresh = useRefresh()
  return useMutation({
    mutationFn: (id: string) => ensureOk(api.DELETE('/api/v1/delegations/received/{id}', { params: { path: { id } } })),
    onSuccess: refresh,
  })
}
