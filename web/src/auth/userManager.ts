import { InMemoryWebStorage, UserManager, WebStorageStateStore } from 'oidc-client-ts'
import type { AuthConfig } from '@/config/runtime'

export function createUserManager(auth: AuthConfig, origin: string): UserManager {
  return new UserManager({
    authority: auth.issuer,
    client_id: auth.client_id,
    redirect_uri: `${origin}/auth/callback`,
    // The hidden-iframe silent sign-in returns to the same route; the page detects it is
    // framed and hands the result back to the parent (see bootstrap.ts).
    silent_redirect_uri: `${origin}/auth/callback`,
    // Exactly what zitadel-bootstrap registers (trailing slash); IdPs compare it verbatim.
    post_logout_redirect_uri: `${origin}/`,
    scope: auth.scopes.join(' '),
    response_type: 'code',
    // Tokens live in memory only; a reload restores the session through the IdP session.
    userStore: new WebStorageStateStore({ store: new InMemoryWebStorage() }),
    automaticSilentRenew: true,
    // A prompt=none round trip that has not answered in 5 s counts as signed out.
    silentRequestTimeoutInSeconds: 5,
    monitorSession: false,
  })
}
