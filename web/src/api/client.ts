import createClient from 'openapi-fetch'
import type { paths } from './schema'
import { ApiError } from './errors'
import { createAuthFetch, type TokenSource } from './authFetch'

export type ApiClient = ReturnType<typeof createApiClient>

export function createApiClient(baseUrl: string, source: TokenSource, baseFetch?: (r: Request) => Promise<Response>) {
  return createClient<paths>({ baseUrl, fetch: createAuthFetch(source, baseFetch) })
}

interface Result<T> {
  data?: T
  error?: unknown
  response: Response
}

export async function unwrap<T>(p: Promise<Result<T>>): Promise<T> {
  const { data, error, response } = await p
  if (error !== undefined || data === undefined) {
    const retry = Number(response.headers.get('Retry-After'))
    throw ApiError.from(response.status, error, Number.isFinite(retry) && retry > 0 ? retry : undefined)
  }
  return data
}

export async function ensureOk(p: Promise<Result<unknown>>): Promise<void> {
  const { error, response } = await p
  if (error !== undefined || !response.ok) throw ApiError.from(response.status, error)
}
