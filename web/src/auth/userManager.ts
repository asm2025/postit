import { InMemoryWebStorage, UserManager, WebStorageStateStore } from 'oidc-client-ts'
import type { AuthConfig } from '@/config/runtime'

export function createUserManager(auth: AuthConfig, origin: string): UserManager {
  return new UserManager({
    authority: auth.issuer,
    client_id: auth.client_id,
    redirect_uri: `${origin}/auth/callback`,
    // oidc-client-ts defaults this to redirect_uri; empty disables its hidden-iframe sign-in,
    // which then fails fast ("No silent_redirect_uri configured") instead of framing the IdP.
    silent_redirect_uri: '',
    // Exactly what zitadel-bootstrap registers (trailing slash); IdPs compare it verbatim.
    post_logout_redirect_uri: `${origin}/`,
    scope: auth.scopes.join(' '),
    response_type: 'code',
    // Tokens live in memory only; a reload restores the session with a top-level
    // prompt=none redirect (SessionProvider), never an iframe.
    userStore: new WebStorageStateStore({ store: new InMemoryWebStorage() }),
    // The state and PKCE verifier of an in-flight sign-in: this tab only, gone when it closes.
    stateStore: new WebStorageStateStore({ store: window.sessionStorage }),
    // Renewal uses the rotating refresh token (offline_access); see tokenSource.ts.
    automaticSilentRenew: true,
    revokeTokensOnSignout: true,
    revokeTokenTypes: ['refresh_token'],
    monitorSession: false,
  })
}
