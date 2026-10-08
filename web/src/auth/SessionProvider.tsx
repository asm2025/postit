import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import type { UserManager } from 'oidc-client-ts'
import { AuthProvider, useAuth } from 'react-oidc-context'
import { authErrorMessage, RETURN_TO_KEY, SessionContext, type Session } from './session'

interface Props {
  userManager: UserManager
  /** Set by the API layer when a 401 could not be renewed. */
  expired: boolean
  children: ReactNode
}

function Adapter({ userManager, expired, children }: Props) {
  const auth = useAuth()
  // A ref, not state: StrictMode runs effects twice in development, and two silent
  // sign-ins would open two iframes.
  const started = useRef(false)
  const [restoring, setRestoring] = useState(true)
  const onCallback = window.location.pathname === '/auth/callback'

  useEffect(() => {
    // On /auth/callback the provider is completing a redirect sign-in itself.
    if (started.current || auth.isLoading || auth.isAuthenticated || onCallback) return
    started.current = true
    // prompt=none in a hidden iframe against the IdP session. login_required,
    // interaction_required or the 5 s timeout leave the app signed out, with no redirect.
    userManager
      .signinSilent()
      .catch(() => undefined)
      .finally(() => {
        setRestoring(false)
      })
  }, [auth.isLoading, auth.isAuthenticated, onCallback, userManager])

  // Only a failed redirect callback is shown; renewal errors surface as 401s instead.
  // react-oidc-context 3.x tags errors with `source`; check the installed version.
  const callbackError =
    auth.error && (auth.error as { source?: string }).source === 'signinCallback' ? auth.error : null

  const session = useMemo<Session>(() => {
    const status = auth.isAuthenticated
      ? 'signedIn'
      : auth.isLoading || (restoring && !onCallback)
        ? 'loading'
        : 'signedOut'
    const innerError = callbackError ? ((callbackError as { innerError?: unknown }).innerError ?? callbackError) : null
    const notice =
      status !== 'signedOut'
        ? null
        : innerError
          ? authErrorMessage(innerError)
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
        void userManager.signinRedirect()
      },
      // Ends the IdP session too; post_logout_redirect_uri brings the user back to `/`.
      signOut: () => {
        void userManager.signoutRedirect()
      },
    }
  }, [auth.isAuthenticated, auth.isLoading, restoring, onCallback, callbackError, expired, userManager])
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
