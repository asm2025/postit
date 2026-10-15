import { authErrorMessage, isSilentRestoreError } from './session'

it('describes IdP error responses and falls back to a generic message', () => {
  expect(authErrorMessage({ error: 'access_denied', error_description: 'User cancelled' })).toMatch(/User cancelled/)
  expect(authErrorMessage({ error: 'server_error' })).toMatch(/server_error/)
  expect(authErrorMessage(new Error('No matching state found in storage'))).toMatch(/could not be completed/i)
  expect(authErrorMessage(undefined)).toBe('Sign-in failed. Try again.')
})

it('silent_errors_show_no_notice: prompt=none refusals are not errors', () => {
  for (const error of ['login_required', 'interaction_required', 'consent_required', 'account_selection_required'])
    expect(isSilentRestoreError({ error })).toBe(true)
  expect(isSilentRestoreError({ error: 'access_denied' })).toBe(false)
  expect(isSilentRestoreError(new Error('state mismatch'))).toBe(false)
})
