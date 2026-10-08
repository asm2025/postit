import { QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactElement } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { ApiContext } from '@/api/ApiContext'
import { createApiClient } from '@/api/client'
import { createQueryClient } from '@/api/queryClient'
import { SessionContext, type Session, type SessionStatus } from '@/auth/session'
import { ConfigContext } from '@/config/ConfigContext'
import { API } from './fixtures'

export interface RenderOptions {
  route?: string
  status?: SessionStatus
  session?: Partial<Session>
  /** Route table; defaults to the element at `*`. */
  routes?: ReactElement
}

export function makeSession(status: SessionStatus, over: Partial<Session> = {}): Session {
  return { status, notice: null, signIn: () => undefined, signOut: () => undefined, ...over }
}

export function renderApp(ui: ReactElement | null, opts: RenderOptions = {}) {
  const queryClient = createQueryClient({ retry: false })
  const api = createApiClient(API, {
    getAccessToken: () => 'test-token',
    renew: () => Promise.resolve(null),
    signOut: () => undefined,
  })
  const session = makeSession(opts.status ?? 'signedIn', opts.session)
  return render(
    <QueryClientProvider client={queryClient}>
      <ConfigContext.Provider
        value={{
          runtime: { API_BASE_URL: API, ENV: 'development' },
          auth: { issuer: 'https://idp.test', client_id: 'c', scopes: ['openid'] },
        }}
      >
        <ApiContext.Provider value={api}>
          <SessionContext.Provider value={session}>
            <MemoryRouter initialEntries={[opts.route ?? '/']}>
              {opts.routes ?? (
                <Routes>
                  <Route path="*" element={ui} />
                </Routes>
              )}
            </MemoryRouter>
          </SessionContext.Provider>
        </ApiContext.Provider>
      </ConfigContext.Provider>
    </QueryClientProvider>,
  )
}
