import { useQuery } from '@tanstack/react-query'
import { unwrap } from '@/api/client'
import { useApi } from '@/api/ApiContext'
import type { components } from '@/api/schema'

export type Me = components['schemas']['MeDto']

export function useMe(enabled = true) {
  const api = useApi()
  return useQuery({
    queryKey: ['me'],
    queryFn: () => unwrap(api.GET('/api/v1/me')),
    enabled,
  })
}
