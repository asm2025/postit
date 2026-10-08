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

const MESSAGES: Partial<Record<string, string>> = {
  unauthenticated: 'Your session expired. Sign in again.',
  account_pending: 'Your account is waiting for approval.',
  account_disabled: 'Your account is disabled.',
  forbidden: 'You are not allowed to do that.',
  not_found: 'That item no longer exists.',
  validation_failed: 'The request was not valid.',
  last_admin: 'That would remove the last active admin.',
  user_deleting: 'That user is being deleted.',
  rate_limited: 'Too many requests. Wait a moment and try again.',
  version_conflict: 'Someone else changed this. Reload and try again.',
  precondition_required: 'Reload and try again.',
  idempotency_key_reused: 'That request was already used with different data.',
  idempotency_in_progress: 'That request is already running.',
  request_timeout: 'The request took too long. Try again.',
  payload_too_large: 'That is too large to send.',
  unavailable: 'The service is starting up. Try again shortly.',
  internal: 'Something went wrong on the server.',
}

export function messageFor(error: unknown): string {
  if (error instanceof ApiError) {
    return MESSAGES[error.code] ?? error.detail ?? 'Something went wrong. Try again.'
  }
  return 'Something went wrong. Try again.'
}
