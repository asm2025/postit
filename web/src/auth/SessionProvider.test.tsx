import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { StrictMode } from 'react'
import { vi } from 'vitest'
import arCommon from '@/i18n/ar/common.json'
import { setLocale } from '@/i18n'
import { useSession } from './session'
import { SessionProvider } from './SessionProvider'

// Every `events.addX` records its handler and returns an unsubscribe function, as
// oidc-client-ts does; `fire` calls the handlers registered for one event.
function fakeEvents() {
  const handlers: Record<string, ((...args: unknown[]) => void)[]> = {}
  const events = new Proxy(
    {},
    {
      get: (_, name: string) => (handler: (...args: unknown[]) => void) => {
        ;(handlers[name] ??= []).push(handler)
        return () => undefined
      },
    },
  )
  const fire = (name: string, ...args: unknown[]) => {
    for (const handler of handlers[name] ?? []) handler(...args)
  }
  return { events, fire }
}

function fakeManager({ signedIn = false } = {}) {
  const user = signedIn ? { access_token: 'a', expired: false, profile: { sub: 's' } } : null
  const { events, fire } = fakeEvents()
  return {
    settings: {},
    events,
    fire,
    getUser: vi.fn(() => Promise.resolve(user)),
    signinSilent: vi.fn(() => Promise.reject(new Error('login_required'))),
    signinRedirect: vi.fn(() => Promise.resolve(undefined)),
    signoutRedirect: vi.fn(() => Promise.resolve(undefined)),
    removeUser: vi.fn(() => Promise.resolve(undefined)),
  }
}

function Probe() {
  const { status, notice } = useSession()
  return (
    <p>
      {status}|{notice ?? ''}
    </p>
  )
}

function SignOutButton() {
  const s = useSession()
  return (
    <button
      onClick={() => {
        s.signOut()
      }}
    >
      sign out
    </button>
  )
}

it('restores once per tab with a top-level prompt=none redirect, even under StrictMode', async () => {
  sessionStorage.clear()
  const um = fakeManager()
  render(
    <StrictMode>
      <SessionProvider userManager={um as never} expired={false}>
        <Probe />
      </SessionProvider>
    </StrictMode>,
  )
  await vi.waitFor(() => {
    expect(um.signinRedirect).toHaveBeenCalledTimes(1)
  })
  expect(um.signinRedirect).toHaveBeenCalledWith({ prompt: 'none', extraQueryParams: { ui_locales: 'en-US' } })
  expect(um.signinSilent).not.toHaveBeenCalled()
  expect(sessionStorage.getItem('postit.restoreAttempted')).toBe('1')
})

it('does not redirect again once a restore was attempted in this tab', async () => {
  sessionStorage.setItem('postit.restoreAttempted', '1')
  const um = fakeManager()
  render(
    <SessionProvider userManager={um as never} expired={false}>
      <Probe />
    </SessionProvider>,
  )
  expect(await screen.findByText('signedOut|')).toBeInTheDocument()
  expect(um.signinRedirect).not.toHaveBeenCalled()
})

it('signing out marks the tab as restored, so the post-logout landing makes no hop', async () => {
  sessionStorage.clear()
  const um = fakeManager({ signedIn: true })
  render(
    <SessionProvider userManager={um as never} expired={false}>
      <SignOutButton />
    </SessionProvider>,
  )
  await userEvent.click(await screen.findByRole('button', { name: 'sign out' }))
  expect(sessionStorage.getItem('postit.restoreAttempted')).toBe('1')
  expect(um.signoutRedirect).toHaveBeenCalledTimes(1)
})

it('tells the user their session expired', async () => {
  sessionStorage.setItem('postit.restoreAttempted', '1')
  render(
    <SessionProvider userManager={fakeManager() as never} expired>
      <Probe />
    </SessionProvider>,
  )
  expect(await screen.findByText(/signedOut\|Your session expired/)).toBeInTheDocument()
})

it('a session lost while signed in shows the expired notice and makes no automatic redirect', async () => {
  sessionStorage.clear()
  const um = fakeManager({ signedIn: true })
  const { rerender } = render(
    <SessionProvider userManager={um as never} expired={false}>
      <Probe />
    </SessionProvider>,
  )
  expect(await screen.findByText('signedIn|')).toBeInTheDocument()
  // What authFetch does after a 401 it cannot renew: drop the user, report expired.
  act(() => {
    um.fire('addUserUnloaded')
  })
  rerender(
    <SessionProvider userManager={um as never} expired>
      <Probe />
    </SessionProvider>,
  )
  expect(await screen.findByText(/signedOut\|Your session expired/)).toBeInTheDocument()
  expect(um.signinRedirect).not.toHaveBeenCalled()
})

it('shows the expired notice in the screen language', async () => {
  sessionStorage.setItem('postit.restoreAttempted', '1')
  await setLocale('ar')
  try {
    render(
      <SessionProvider userManager={fakeManager() as never} expired>
        <Probe />
      </SessionProvider>,
    )
    expect(await screen.findByText(new RegExp(arCommon.errors.unauthenticated))).toBeInTheDocument()
  } finally {
    await setLocale('en-US')
  }
})
