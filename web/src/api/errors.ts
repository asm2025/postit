import i18next from 'i18next'
import enCommon from '@/i18n/en-US/common.json'

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly detail?: string
  readonly retryAfter?: number

  constructor(status: number, code: string, detail?: string, retryAfter?: number) {
    super(detail ?? code)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.detail = detail
    this.retryAfter = retryAfter
  }

  static from(status: number, body: unknown, retryAfter?: number): ApiError {
    if (body && typeof body === 'object') {
      const b = body as { code?: unknown; detail?: unknown }
      const code = typeof b.code === 'string' ? b.code : 'unknown'
      const detail = typeof b.detail === 'string' ? b.detail : undefined
      return new ApiError(status, code, detail, retryAfter)
    }
    return new ApiError(status, 'unknown', undefined, retryAfter)
  }
}

/** A translated message for the error; never the English `detail`, which is for developers. */
export function messageFor(error: unknown): string {
  const known = error instanceof ApiError && Object.hasOwn(enCommon.errors, error.code)
  const code = known ? (error.code as keyof typeof enCommon.errors) : 'generic'
  return i18next.t(`errors.${code}`)
}
