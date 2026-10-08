export interface TokenSource {
  getAccessToken: () => string | null
  /** Resolves the new access token, or null when the session cannot be renewed. */
  renew: () => Promise<string | null>
  signOut: () => void
}

function withToken(request: Request, token: string | null): Request {
  const next = new Request(request)
  if (token) next.headers.set('Authorization', `Bearer ${token}`)
  return next
}

export function createAuthFetch(
  source: TokenSource,
  baseFetch: (r: Request) => Promise<Response> = (r) => fetch(r),
): (input: Request) => Promise<Response> {
  return async (input) => {
    const spare = input.clone()
    const first = await baseFetch(withToken(input, source.getAccessToken()))
    if (first.status !== 401) return first
    const token = await source.renew()
    if (!token) {
      source.signOut()
      return first
    }
    const retry = await baseFetch(withToken(spare, token))
    if (retry.status === 401) source.signOut()
    return retry
  }
}
