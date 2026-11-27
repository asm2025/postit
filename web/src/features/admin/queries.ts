import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'

export type AdminPlatform = components['schemas']['AdminPlatformDto']
export type PublishingSettings = components['schemas']['PublishingSettingsDto']
export type UserLimits = components['schemas']['UserLimitsDto']
export type UserLimitsInput = components['schemas']['PutUserLimitsRequest']

// Admin data is the same in every workspace and the admin routes never carry act-as, so these keys
// start with `admin` and are not workspace keys.

export function useAdminPlatforms() {
  const api = useApi()
  return useQuery({
    queryKey: ['admin', 'platforms'],
    queryFn: () => unwrap(api.GET('/api/v1/admin/platforms')),
  })
}

/** Switches a platform on or off. People see the change in the connect screen's list too. */
export function useSetPlatformEnabled() {
  const api = useApi()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      unwrap(api.PUT('/api/v1/admin/platforms/{id}', { params: { path: { id } }, body: { enabled } })),
    onSuccess: () =>
      Promise.all([
        qc.invalidateQueries({ queryKey: ['admin', 'platforms'] }),
        qc.invalidateQueries({ queryKey: ['platforms'] }),
      ]),
  })
}

export function usePublishingSettings() {
  const api = useApi()
  return useQuery({
    queryKey: ['admin', 'publishing'],
    queryFn: () => unwrap(api.GET('/api/v1/admin/settings/publishing')),
  })
}

/** The live switch. While it is off live deliveries park; dry runs still run. */
export function useSetLive() {
  const api = useApi()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (live: boolean) => unwrap(api.PUT('/api/v1/admin/settings/publishing', { body: { live } })),
    onSuccess: (saved) => {
      qc.setQueryData(['admin', 'publishing'], saved)
    },
  })
}

export function useUserLimits(id: string) {
  const api = useApi()
  return useQuery({
    queryKey: ['admin', 'limits', id],
    queryFn: () => unwrap(api.GET('/api/v1/admin/users/{id}/limits', { params: { path: { id } } })),
  })
}

export function useSaveUserLimits(id: string) {
  const api = useApi()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: UserLimitsInput) =>
      unwrap(api.PUT('/api/v1/admin/users/{id}/limits', { params: { path: { id } }, body })),
    onSuccess: (saved) => {
      qc.setQueryData(['admin', 'limits', id], saved)
    },
  })
}
