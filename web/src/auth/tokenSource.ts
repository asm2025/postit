import type { UserManager } from 'oidc-client-ts'
import type { TokenSource } from '@/api/authFetch'
import { singleFlight } from '@/lib/singleFlight'

type Manager = Pick<UserManager, 'getUser' | 'signinSilent' | 'removeUser' | 'events'>

export function createTokenSource(um: Manager, onSignedOut: () => void): TokenSource {
  let token: string | null = null
  um.events.addUserLoaded((u) => {
    token = u.access_token
  })
  um.events.addUserUnloaded(() => {
    token = null
  })
  void um.getUser().then((u) => {
    token ??= u?.access_token ?? null
  })
  const renew = singleFlight(async () => {
    try {
      // Refresh-token renewal only: without a refresh token oidc-client-ts would fall back
      // to a hidden iframe, which the CSP forbids.
      const current = await um.getUser()
      if (!current?.refresh_token) {
        token = null
        return token
      }
      const u = await um.signinSilent()
      token = u?.access_token ?? null
    } catch {
      token = null
    }
    return token
  })
  return {
    getAccessToken: () => token,
    renew,
    signOut: () => {
      token = null
      void um.removeUser().finally(onSignedOut)
    },
  }
}
