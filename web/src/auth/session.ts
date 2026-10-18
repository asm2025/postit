import i18next from 'i18next'
import { createContext, useContext } from 'react'
import { isolate } from '@/i18n/bidi'

export type SessionStatus = 'loading' | 'signedOut' | 'signedIn'

export interface Session {
  status: SessionStatus
  /** Why the user is signed out, for the Sign-in page: an IdP error or an expired session. */
  notice: string | null
  signIn: (returnTo?: string) => void
  signOut: () => void
}

export const RETURN_TO_KEY = 'postit.returnTo'

/** Set once this tab has tried a prompt=none restore; holds no token. */
export const RESTORE_KEY = 'postit.restoreAttempted'

const SILENT_ERRORS = new Set([
  'login_required',
  'interaction_required',
  'consent_required',
  'account_selection_required',
])

/** A prompt=none restore that found no usable IdP session: signed out, not an error. */
export function isSilentRestoreError(error: unknown): boolean {
  const code = (error ?? {}) as { error?: unknown }
  return typeof code.error === 'string' && SILENT_ERRORS.has(code.error)
}

export const SessionContext = createContext<Session | null>(null)

export function useSession(): Session {
  const session = useContext(SessionContext)
  if (!session) throw new Error('useSession must be used inside a SessionProvider')
  return session
}

/**
 * A short message for a failed sign-in callback (IdP error response or state mismatch). The
 * wrapper text comes from the catalog in the screen language; text the IdP sent is kept as
 * sent and isolated, so its direction cannot reorder the sentence around it.
 */
export function authErrorMessage(error: unknown): string {
  const e = (error ?? {}) as { error?: unknown; error_description?: unknown; message?: unknown }
  const description = typeof e.error_description === 'string' ? isolate(e.error_description) : null
  if (typeof e.error === 'string') {
    if (e.error === 'access_denied')
      return description ? i18next.t('auth.deniedWithDetail', { detail: description }) : i18next.t('auth.denied')
    return description
      ? i18next.t('auth.failedWithDetail', { detail: description })
      : i18next.t('auth.failedWithCode', { code: isolate(e.error) })
  }
  if (typeof e.message === 'string' && /state/i.test(e.message)) return i18next.t('auth.incomplete')
  return i18next.t('auth.failed')
}
