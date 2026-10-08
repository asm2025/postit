import { vi } from 'vitest'
import { createTokenSource } from './tokenSource'

function fakeManager(user: { access_token: string } | null) {
  const handlers: Record<string, (u?: unknown) => void> = {}
  return {
    handlers,
    um: {
      getUser: vi.fn(() => Promise.resolve(user)),
      signinSilent: vi.fn(() => Promise.resolve({ access_token: 'renewed' })),
      removeUser: vi.fn(() => Promise.resolve(undefined)),
      events: {
        addUserLoaded: (h: (u: unknown) => void) => {
          handlers.loaded = h
        },
        addUserUnloaded: (h: () => void) => {
          handlers.unloaded = h
        },
      },
    },
  }
}

it('tracks the access token from the manager', async () => {
  const { um, handlers } = fakeManager({ access_token: 'a' })
  const src = createTokenSource(um as never, vi.fn())
  await Promise.resolve()
  await Promise.resolve()
  expect(src.getAccessToken()).toBe('a')
  handlers.loaded({ access_token: 'b' })
  expect(src.getAccessToken()).toBe('b')
  handlers.unloaded()
  expect(src.getAccessToken()).toBeNull()
})

it('renews through one shared silent sign-in', async () => {
  const { um } = fakeManager({ access_token: 'a' })
  const src = createTokenSource(um as never, vi.fn())
  const [x, y] = await Promise.all([src.renew(), src.renew()])
  expect([x, y]).toEqual(['renewed', 'renewed'])
  expect(um.signinSilent).toHaveBeenCalledTimes(1)
})

it('returns null when the silent sign-in fails', async () => {
  const { um } = fakeManager(null)
  um.signinSilent.mockRejectedValueOnce(new Error('login_required'))
  const src = createTokenSource(um as never, vi.fn())
  expect(await src.renew()).toBeNull()
})

it('signOut clears the user and notifies', async () => {
  const { um } = fakeManager({ access_token: 'a' })
  const onSignedOut = vi.fn()
  const src = createTokenSource(um as never, onSignedOut)
  src.signOut()
  await Promise.resolve()
  expect(um.removeUser).toHaveBeenCalled()
  expect(onSignedOut).toHaveBeenCalled()
})
