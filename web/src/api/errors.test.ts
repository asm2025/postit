import { ApiError, messageFor } from './errors'

it('parses problem+json bodies', () => {
  const e = ApiError.from(409, { code: 'last_admin', detail: 'nope', status: 409 })
  expect(e).toMatchObject({ status: 409, code: 'last_admin', detail: 'nope' })
})

it('tolerates non-problem bodies', () => {
  const e = ApiError.from(502, 'Bad Gateway')
  expect(e.code).toBe('unknown')
  expect(e.status).toBe(502)
})

it('maps known codes to user-facing text and falls back to a generic message, never the detail', () => {
  expect(messageFor(ApiError.from(409, { code: 'last_admin' }))).toMatch(/last active admin/i)
  expect(messageFor(ApiError.from(422, { code: 'something_new', detail: 'custom detail' }))).toBe(
    'Something went wrong. Try again.',
  )
  expect(messageFor(new Error('boom'))).toBe('Something went wrong. Try again.')
})

it('explains the push problems', () => {
  expect(messageFor(ApiError.from(404, { code: 'push_disabled' }))).toBe(
    "Push notifications aren't available on this server.",
  )
  expect(messageFor(ApiError.from(422, { code: 'push_endpoint_invalid', field: 'p256dh' }))).toMatch(
    /couldn't be registered for notifications/,
  )
})

it('explains a job action the job can no longer take', () => {
  expect(messageFor(ApiError.from(409, { code: 'job_action_not_allowed', detail: 'x' }))).toBe(
    "That job can't take this action right now. It may have changed; refresh and try again.",
  )
})

it('explains the contact problems', () => {
  expect(messageFor(ApiError.from(413, { code: 'csv_too_large' }))).toBe(
    'That file is too large. Split it into smaller files.',
  )
  expect(messageFor(ApiError.from(400, { code: 'consent_attestation_required' }))).toMatch(
    /permission to email these contacts/,
  )
  expect(messageFor(ApiError.from(409, { code: 'suppression_not_removable' }))).toMatch(
    /Only a bounced address can be removed/,
  )
})

it('explains the test email problems', () => {
  expect(messageFor(ApiError.from(409, { code: 'email_address_missing' }))).toBe(
    'Your profile has no verified email address to send the test to.',
  )
  expect(messageFor(ApiError.from(409, { code: 'live_publish_disabled' }))).toBe(
    'Live publishing is switched off on this server. Ask an admin to turn it on.',
  )
  expect(messageFor(ApiError.from(409, { code: 'test_send_refused', detail: 'smtp_5_1_1' }))).toBe(
    "The mail server refused your address, so the test email wasn't delivered.",
  )
  expect(messageFor(ApiError.from(409, { code: 'capability_unsupported' }))).toBe("This platform doesn't support that.")
})
