import { User } from 'oidc-client-ts'
import { createUserManager } from './userManager'

const auth = { issuer: 'https://idp.test', client_id: 'postit-app', scopes: ['openid', 'offline_access'] }

it('never frames: no silent redirect URI, and revokes the refresh token on sign-out', () => {
  const um = createUserManager(auth, 'https://app.test')
  expect(um.settings.silent_redirect_uri).toBeFalsy()
  expect(um.settings.revokeTokensOnSignout).toBe(true)
  expect(um.settings.revokeTokenTypes).toEqual(['refresh_token'])
})

it('keeps tokens out of web storage', async () => {
  localStorage.clear()
  sessionStorage.clear()
  const um = createUserManager(auth, 'https://app.test')
  await um.storeUser(
    new User({
      access_token: 'secret-access-token',
      refresh_token: 'secret-refresh-token',
      id_token: 'secret-id-token',
      token_type: 'Bearer',
      profile: { sub: 's', iss: 'https://idp.test', aud: 'postit-app', exp: 0, iat: 0 },
    }),
  )
  const dump = [localStorage, sessionStorage]
    .flatMap((store) => Object.keys(store).map((k) => store.getItem(k) ?? ''))
    .join(' ')
  expect(dump).not.toContain('secret-')
})

it('keeps sign-in state in sessionStorage, not localStorage', () => {
  const um = createUserManager(auth, 'https://app.test')
  // WebStorageStateStore keeps its backing store in the private `_store` field (3.5.0).
  expect((um.settings.stateStore as unknown as { _store: Storage })._store).toBe(window.sessionStorage)
})
