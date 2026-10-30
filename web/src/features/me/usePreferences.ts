import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrap } from '@/api/client'
import { useApi } from '@/api/ApiContext'
import type { components } from '@/api/schema'

export type Preferences = components['schemas']['PreferencesDto']
export type PreferencesUpdate = components['schemas']['UpdatePreferences']

/** Shared with `LocaleSync`, which fills the same cache entry: it always holds a full `PreferencesDto`. */
export const PREFERENCES_KEY = ['preferences'] as const

export function usePreferences(enabled = true) {
  const api = useApi()
  return useQuery({
    queryKey: PREFERENCES_KEY,
    queryFn: () => unwrap(api.GET('/api/v1/me/preferences')),
    enabled,
    staleTime: Infinity,
  })
}

/**
 * Saves a partial change. The server answers with the full preferences, which replace the cache, so the
 * cache never holds a partial object. Changes run one after another, so a slow response cannot undo a later one.
 */
export function useUpdatePreferences() {
  const api = useApi()
  const queryClient = useQueryClient()
  return useMutation({
    scope: { id: 'preferences' },
    mutationFn: (body: PreferencesUpdate) => unwrap(api.PUT('/api/v1/me/preferences', { body })),
    onSuccess: (saved) => {
      queryClient.setQueryData(PREFERENCES_KEY, saved)
    },
  })
}
