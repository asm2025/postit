import i18next from 'i18next'
import enCommon from '@/i18n/en-US/common.json'

/** One rule a request broke: the code to translate, its parameters, and where (`options.<field>`). */
export interface ProblemViolation {
  code: string
  params?: Record<string, unknown>
  path?: string | null
}

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly detail?: string
  readonly retryAfter?: number
  /** Every rule a `validation_failed` answer says the request broke. */
  readonly violations: ProblemViolation[]
  /** The field a `credentials_invalid` (or similar) answer points at. */
  readonly field?: string

  constructor(
    status: number,
    code: string,
    detail?: string,
    retryAfter?: number,
    extra: { violations?: ProblemViolation[]; field?: string } = {},
  ) {
    super(detail ?? code)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.detail = detail
    this.retryAfter = retryAfter
    this.violations = extra.violations ?? []
    this.field = extra.field
  }

  static from(status: number, body: unknown, retryAfter?: number): ApiError {
    if (body && typeof body === 'object') {
      const b = body as { code?: unknown; detail?: unknown; violations?: unknown; field?: unknown }
      const code = typeof b.code === 'string' ? b.code : 'unknown'
      const detail = typeof b.detail === 'string' ? b.detail : undefined
      const violations = Array.isArray(b.violations)
        ? (b.violations as unknown[]).filter(
            (v): v is ProblemViolation =>
              typeof v === 'object' && v !== null && typeof (v as { code?: unknown }).code === 'string',
          )
        : []
      return new ApiError(status, code, detail, retryAfter, {
        violations,
        field: typeof b.field === 'string' ? b.field : undefined,
      })
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
