import { currentLocale } from '@/i18n'
import { getActAs } from '@/workspace/store'
import { needsActAs } from './actAs'

export interface TokenSource {
  getAccessToken: () => string | null
  /** Resolves the new access token, or null when the session cannot be renewed. */
  renew: () => Promise<string | null>
  signOut: () => void
}

function withToken(request: Request, token: string | null, owner: string | null): Request {
  const next = new Request(request)
  next.headers.set('Accept-Language', currentLocale())
  if (token) next.headers.set('Authorization', `Bearer ${token}`)
  if (owner && needsActAs(request.url)) next.headers.set('X-Postit-Act-As', owner)
  return next
}

export function createAuthFetch(
  source: TokenSource,
  baseFetch: (r: Request) => Promise<Response> = (r) => fetch(r),
  actAs: () => string | null = getActAs,
): (input: Request) => Promise<Response> {
  return async (input) => {
    // Read once, so a retry after renewal goes to the workspace the first try did.
    const owner = actAs()
    const spare = input.clone()
    const first = await baseFetch(withToken(input, source.getAccessToken(), owner))
    if (first.status !== 401) return first
    const token = await source.renew()
    if (!token) {
      source.signOut()
      return first
    }
    const retry = await baseFetch(withToken(spare, token, owner))
    if (retry.status === 401) source.signOut()
    return retry
  }
}
