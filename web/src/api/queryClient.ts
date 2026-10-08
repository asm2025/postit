import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'
import { ApiError } from './errors'

/** Codes that mean the caller's own status or role changed: re-check `/me` so the gate moves them. */
const RECHECK_ME = new Set(['account_pending', 'account_disabled', 'forbidden'])

/**
 * The app's query client (tests pass `retry: false`). `/me`'s own errors are left to the
 * gate; re-fetching `/me` from its own failure would loop.
 */
export function createQueryClient({ retry = true }: { retry?: boolean } = {}): QueryClient {
  const recheck = (error: unknown, key: unknown) => {
    if (key !== 'me' && error instanceof ApiError && RECHECK_ME.has(error.code)) {
      void client.invalidateQueries({ queryKey: ['me'] })
    }
  }
  const client: QueryClient = new QueryClient({
    queryCache: new QueryCache({
      onError: (error, query) => {
        recheck(error, query.queryKey[0])
      },
    }),
    mutationCache: new MutationCache({
      onError: (error) => {
        recheck(error, undefined)
      },
    }),
    defaultOptions: {
      queries: {
        retry: retry ? (count, error) => !(error instanceof ApiError && error.status < 500) && count < 2 : false,
        refetchOnWindowFocus: false,
      },
      mutations: { retry: false },
    },
  })
  return client
}
