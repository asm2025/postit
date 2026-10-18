import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { setLocale } from '@/i18n'
import arCommon from '@/i18n/ar/common.json'
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

describe('Arabic', () => {
  beforeEach(async () => {
    await setLocale('ar')
  })
  afterEach(async () => {
    await setLocale('en-US')
  })

  it('wraps the IdP text in the catalog sentence and keeps the IdP text as sent', () => {
    expect(authErrorMessage(undefined)).toBe(arCommon.auth.failed)
    expect(authErrorMessage({ error: 'access_denied' })).toBe(arCommon.auth.denied)
    const message = authErrorMessage({ error: 'access_denied', error_description: 'User cancelled' })
    expect(message).toContain('User cancelled')
    expect(message).not.toMatch(/Sign-in/)
    expect(authErrorMessage({ error: 'server_error' })).toContain('server_error')
  })
})
