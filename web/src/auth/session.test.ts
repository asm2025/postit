import { authErrorMessage } from './session'

it('describes IdP error responses and falls back to a generic message', () => {
  expect(authErrorMessage({ error: 'access_denied', error_description: 'User cancelled' })).toMatch(/User cancelled/)
  expect(authErrorMessage({ error: 'server_error' })).toMatch(/server_error/)
  expect(authErrorMessage(new Error('No matching state found in storage'))).toMatch(/could not be completed/i)
  expect(authErrorMessage(undefined)).toBe('Sign-in failed. Try again.')
})
