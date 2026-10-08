import { render, screen } from '@testing-library/react'
import { StrictMode } from 'react'
import { vi } from 'vitest'
import { useSession } from './session'
import { SessionProvider } from './SessionProvider'

// Every `events.addX` returns an unsubscribe function, as oidc-client-ts does.
const events = new Proxy({}, { get: () => () => () => undefined })

function fakeManager() {
  return {
    settings: {},
    events,
    getUser: vi.fn(() => Promise.resolve(null)),
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

it('tries one silent sign-in on load, even under StrictMode, then reports signed out', async () => {
  const um = fakeManager()
  render(
    <StrictMode>
      <SessionProvider userManager={um as never} expired={false}>
        <Probe />
      </SessionProvider>
    </StrictMode>,
  )
  expect(await screen.findByText('signedOut|')).toBeInTheDocument()
  expect(um.signinSilent).toHaveBeenCalledTimes(1)
})

it('tells the user their session expired', async () => {
  render(
    <SessionProvider userManager={fakeManager() as never} expired>
      <Probe />
    </SessionProvider>,
  )
  expect(await screen.findByText(/signedOut\|Your session expired/)).toBeInTheDocument()
})
