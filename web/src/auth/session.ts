import { createContext, useContext } from 'react'

export type SessionStatus = 'loading' | 'signedOut' | 'signedIn'

export interface Session {
  status: SessionStatus
  /** Why the user is signed out, for the Sign-in page: an IdP error or an expired session. */
  notice: string | null
  signIn: (returnTo?: string) => void
  signOut: () => void
}

export const RETURN_TO_KEY = 'postit.returnTo'

export const SessionContext = createContext<Session | null>(null)

export function useSession(): Session {
  const session = useContext(SessionContext)
  if (!session) throw new Error('useSession must be used inside a SessionProvider')
  return session
}

/** A short message for a failed sign-in callback (IdP error response or state mismatch). */
export function authErrorMessage(error: unknown): string {
  const e = (error ?? {}) as { error?: unknown; error_description?: unknown; message?: unknown }
  const description = typeof e.error_description === 'string' ? e.error_description : null
  if (typeof e.error === 'string') {
    if (e.error === 'access_denied') return `Sign-in was cancelled or denied${description ? `: ${description}` : '.'}`
    return description ? `Sign-in failed: ${description}` : `Sign-in failed (${e.error}).`
  }
  if (typeof e.message === 'string' && /state/i.test(e.message)) return 'Sign-in could not be completed. Try again.'
  return 'Sign-in failed. Try again.'
}
