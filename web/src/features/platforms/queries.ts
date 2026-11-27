import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'

export type Platform = components['schemas']['AgentDescriptor']

const FIVE_MINUTES = 5 * 60_000

/**
 * The platforms accounts can be connected to here, in the order to show them. The list is the
 * same in every workspace, so it is not keyed by one and the request goes out without act-as.
 */
export function usePlatforms() {
  const api = useApi()
  return useQuery({
    queryKey: ['platforms'],
    queryFn: () => unwrap(api.GET('/api/v1/platforms')),
    staleTime: FIVE_MINUTES,
  })
}

/** One platform of that list, or undefined while it loads or when the deployment does not offer it. */
export function usePlatform(id: string | undefined): Platform | undefined {
  const platforms = usePlatforms()
  return id === undefined ? undefined : platforms.data?.find((p) => p.platform === id)
}

// Object URLs are released when their query leaves the cache, once per client.
const watched = new WeakSet<QueryClient>()

function releaseOnRemoval(client: QueryClient) {
  if (watched.has(client)) return
  watched.add(client)
  client.getQueryCache().subscribe((event) => {
    const key = event.query.queryKey as readonly unknown[]
    if (event.type === 'removed' && key[0] === 'platformIcon') {
      const url = event.query.state.data as unknown
      if (typeof url === 'string') URL.revokeObjectURL(url)
    }
  })
}

/**
 * An object URL for the platform's icon. The icon route needs the bearer token, so an `<img>`
 * cannot load it by address; the PNG is fetched through the API client and kept for good.
 */
export function usePlatformIcon(id: string | undefined): string | undefined {
  const api = useApi()
  const query = useQuery({
    queryKey: ['platformIcon', id],
    enabled: id !== undefined,
    staleTime: Infinity,
    queryFn: async () => {
      const blob = await unwrap(
        api.GET('/api/v1/platforms/{id}/icon', { params: { path: { id: id ?? '' } }, parseAs: 'blob' }),
      )
      return URL.createObjectURL(blob)
    },
  })
  const client = useQueryClient()
  useEffect(() => {
    releaseOnRemoval(client)
  }, [client])
  return query.data
}
