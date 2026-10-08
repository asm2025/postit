export interface RuntimeConfig {
  API_BASE_URL: string
  ENV: string
}

export interface AuthConfig {
  issuer: string
  client_id: string
  scopes: string[]
}

type FetchFn = (input: string, init?: RequestInit) => Promise<Response>

async function getJson(fetchFn: FetchFn, url: string): Promise<unknown> {
  const res = await fetchFn(url, { headers: { Accept: 'application/json' } })
  if (!res.ok) throw new Error(`GET ${url} failed with ${String(res.status)}`)
  return res.json()
}

export async function loadRuntimeConfig(fetchFn: FetchFn = fetch): Promise<RuntimeConfig> {
  const body = (await getJson(fetchFn, '/config.json')) as Partial<RuntimeConfig>
  if (!body.API_BASE_URL) throw new Error('config.json is missing API_BASE_URL')
  return { API_BASE_URL: body.API_BASE_URL.replace(/\/+$/, ''), ENV: body.ENV ?? 'development' }
}

export async function loadAuthConfig(apiBase: string, fetchFn: FetchFn = fetch): Promise<AuthConfig> {
  const body = (await getJson(fetchFn, `${apiBase}/api/v1/auth/config`)) as Partial<AuthConfig>
  if (!body.issuer || !body.client_id || !body.scopes) throw new Error('auth config is incomplete')
  return { issuer: body.issuer, client_id: body.client_id, scopes: body.scopes }
}
