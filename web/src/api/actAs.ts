/** First path segments of the routes that act for the signed-in user, never for a workspace. */
const SELF_SEGMENTS = new Set(['me', 'notifications', 'push', 'users', 'admin', 'auth', 'platforms'])

/**
 * Whether a request goes out with `X-Postit-Act-As` while a workspace is active. The routes that
 * refuse the header answer `owner_only` if it is sent, and workspace routes act for the wrong
 * person without it. Matches whole path segments, never a prefix: `me` is not `media`.
 */
export function needsActAs(url: string): boolean {
  let pathname: string
  try {
    pathname = new URL(url).pathname
  } catch {
    return false
  }
  const prefix = '/api/v1/'
  if (!pathname.startsWith(prefix)) return false
  const [first, second] = pathname.slice(prefix.length).split('/')
  if (SELF_SEGMENTS.has(first)) return false
  // The caller's grants from others, and the AI presets: the same for every workspace.
  if (first === 'delegations' && second === 'received') return false
  if (first === 'ai' && second === 'presets') return false
  return true
}
