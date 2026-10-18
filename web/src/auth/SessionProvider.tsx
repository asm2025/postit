import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import type { UserManager } from 'oidc-client-ts'
import { AuthProvider, useAuth } from 'react-oidc-context'
import { currentLocale } from '@/i18n'
import {
  authErrorMessage,
  isSilentRestoreError,
  RESTORE_KEY,
  RETURN_TO_KEY,
  SessionContext,
  type Session,
} from './session'

interface Props {
  userManager: UserManager
  /** Set by the API layer when a 401 could not be renewed. */
  expired: boolean
  children: ReactNode
}

function Adapter({ userManager, expired, children }: Props) {
  const auth = useAuth()
  // A ref, not state: StrictMode runs effects twice in development, and two restores
  // would redirect twice.
  const started = useRef(false)
  const [restoring, setRestoring] = useState(true)
  const onCallback = window.location.pathname === '/auth/callback'
  // Once signed in, losing the session later in this page (a 401 that could not be renewed)
  // must not restore automatically: the user sees the expired notice, and an IdP whose
  // tokens the API refuses cannot loop. Set during render, React's derived-state pattern.
  const [wasSignedIn, setWasSignedIn] = useState(false)
  if (auth.isAuthenticated && !wasSignedIn) setWasSignedIn(true)

  useEffect(() => {
    // On /auth/callback the provider is completing a redirect sign-in itself.
    if (started.current || wasSignedIn || auth.isLoading || auth.isAuthenticated || onCallback) return
    started.current = true
    let attempted = true
    try {
      attempted = sessionStorage.getItem(RESTORE_KEY) === '1'
      if (!attempted) {
        sessionStorage.setItem(RESTORE_KEY, '1')
        sessionStorage.setItem(RETURN_TO_KEY, window.location.pathname + window.location.search)
      }
    } catch {
      /* storage blocked: never redirect, so a blocked store cannot loop */
    }
    if (attempted) {
      setRestoring(false)
      return
    }
    // Top-level prompt=none: the IdP answers with a code (session restored) or
    // login_required (signed out). The page navigates away, so `restoring` stays true.
    void userManager.signinRedirect({ prompt: 'none', extraQueryParams: { ui_locales: currentLocale() } }).catch(() => {
      setRestoring(false)
    })
  }, [auth.isLoading, auth.isAuthenticated, wasSignedIn, onCallback, userManager])

  useEffect(() => {
    // A real sign-in makes the next reload eligible for a restore again.
    if (auth.isAuthenticated) {
      try {
        sessionStorage.removeItem(RESTORE_KEY)
      } catch {
        /* ignore */
      }
    }
  }, [auth.isAuthenticated])

  // Only a failed redirect callback is shown; renewal errors surface as 401s instead.
  // react-oidc-context 3.x tags errors with `source`; check the installed version.
  const callbackError =
    auth.error && (auth.error as { source?: string }).source === 'signinCallback' ? auth.error : null

  const session = useMemo<Session>(() => {
    const status = auth.isAuthenticated
      ? 'signedIn'
      : auth.isLoading || (restoring && !onCallback && !wasSignedIn)
        ? 'loading'
        : 'signedOut'
    const innerError = callbackError ? ((callbackError as { innerError?: unknown }).innerError ?? callbackError) : null
    const shown = innerError && !isSilentRestoreError(innerError) ? innerError : null
    const notice =
      status !== 'signedOut'
        ? null
        : shown
          ? authErrorMessage(shown)
          : expired
            ? 'Your session expired. Sign in again.'
            : null
    return {
      status,
      notice,
      signIn: (returnTo) => {
        try {
          sessionStorage.setItem(RETURN_TO_KEY, returnTo ?? '/')
        } catch {
          /* private mode: fall back to the home page */
        }
        void userManager.signinRedirect({ extraQueryParams: { ui_locales: currentLocale() } })
      },
      // Ends the IdP session too; post_logout_redirect_uri brings the user back to `/`.
      // Marks the tab first, so the landing on `/` makes no prompt=none hop (which could
      // silently sign the user back in if the IdP kept its session).
      signOut: () => {
        try {
          sessionStorage.setItem(RESTORE_KEY, '1')
        } catch {
          /* ignore */
        }
        void userManager.signoutRedirect()
      },
    }
  }, [auth.isAuthenticated, auth.isLoading, restoring, wasSignedIn, onCallback, callbackError, expired, userManager])
  return <SessionContext.Provider value={session}>{children}</SessionContext.Provider>
}

/** Drops `code`/`state` from the URL once the redirect sign-in has been processed. */
function clearCallbackParams() {
  window.history.replaceState({}, document.title, window.location.pathname)
}

export function SessionProvider({ userManager, expired, children }: Props) {
  return (
    <AuthProvider userManager={userManager} onSigninCallback={clearCallbackParams}>
      <Adapter userManager={userManager} expired={expired}>
        {children}
      </Adapter>
    </AuthProvider>
  )
}
