export interface RuntimeConfig {
  API_BASE_URL: string
  ENV: string
}

/** Why startup failed, as a code; the screen translates it. */
export type StartupReason = 'runtime_config' | 'auth_config' | 'identity_unreachable'

export class StartupFailure extends Error {
  readonly reason: StartupReason
  constructor(reason: StartupReason, detail?: string) {
    super(detail ?? reason)
    this.name = 'StartupFailure'
    this.reason = reason
  }
}

export interface AuthConfig {
  issuer: string
  client_id: string
  scopes: string[]
}

type FetchFn = (input: string, init?: RequestInit) => Promise<Response>

async function getJson(fetchFn: FetchFn, url: string, reason: StartupReason): Promise<unknown> {
  let res: Response
  try {
    res = await fetchFn(url, { headers: { Accept: 'application/json' } })
  } catch (e) {
    throw new StartupFailure(reason, e instanceof Error ? e.message : undefined)
  }
  if (!res.ok) throw new StartupFailure(reason, `GET ${url} failed with ${String(res.status)}`)
  try {
    return await res.json()
  } catch {
    throw new StartupFailure(reason, `GET ${url} did not answer JSON`)
  }
}

export async function loadRuntimeConfig(fetchFn: FetchFn = fetch): Promise<RuntimeConfig> {
  const body = (await getJson(fetchFn, '/config.json', 'runtime_config')) as Partial<RuntimeConfig>
  if (!body.API_BASE_URL) throw new StartupFailure('runtime_config', 'config.json is missing API_BASE_URL')
  return { API_BASE_URL: body.API_BASE_URL.replace(/\/+$/, ''), ENV: body.ENV ?? 'development' }
}

export async function loadAuthConfig(apiBase: string, fetchFn: FetchFn = fetch): Promise<AuthConfig> {
  const body = (await getJson(fetchFn, `${apiBase}/api/v1/auth/config`, 'auth_config')) as Partial<AuthConfig>
  if (!body.issuer || !body.client_id || !body.scopes)
    throw new StartupFailure('auth_config', 'auth config is incomplete')
  return { issuer: body.issuer, client_id: body.client_id, scopes: body.scopes }
}
