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

it('maps known codes to user-facing text and falls back to the detail', () => {
  expect(messageFor(ApiError.from(409, { code: 'last_admin' }))).toMatch(/last active admin/i)
  expect(messageFor(ApiError.from(422, { code: 'something_new', detail: 'custom detail' }))).toBe('custom detail')
  expect(messageFor(new Error('boom'))).toBe('Something went wrong. Try again.')
})
