import { QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { render } from '@testing-library/react'
import type { ReactElement } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { ApiContext } from '@/api/ApiContext'
import { createApiClient } from '@/api/client'
import { createQueryClient } from '@/api/queryClient'
import { SessionContext, type Session, type SessionStatus } from '@/auth/session'
import { ConfigContext } from '@/config/ConfigContext'
import { setActAs } from '@/workspace/store'
import { WorkspaceScope } from '@/workspace/WorkspaceProvider'
import { API, delegatedWorkspaceFixture, ownWorkspaceFixture, type Workspace } from './fixtures'
import { server } from './server'

export interface RenderOptions {
  route?: string
  status?: SessionStatus
  session?: Partial<Session>
  /** Route table; defaults to the element at `*`. */
  routes?: ReactElement
  /** Acts in this owner's workspace, as a tab that switched to it does. */
  actAs?: string
  /** What `GET /me/workspaces` answers; with `actAs`, the own workspace and a `publish` grant to that owner. */
  workspaces?: Workspace[]
}

export function makeSession(status: SessionStatus, over: Partial<Session> = {}): Session {
  return { status, notice: null, signIn: () => undefined, signOut: () => undefined, ...over }
}

export function renderApp(ui: ReactElement | null, opts: RenderOptions = {}) {
  const queryClient = createQueryClient({ retry: false })
  if (opts.actAs !== undefined) setActAs(opts.actAs)
  const workspaces =
    opts.workspaces ?? (opts.actAs ? [ownWorkspaceFixture(), delegatedWorkspaceFixture(opts.actAs)] : null)
  if (workspaces) server.use(http.get(`${API}/api/v1/me/workspaces`, () => HttpResponse.json(workspaces)))
  const api = createApiClient(API, {
    getAccessToken: () => 'test-token',
    renew: () => Promise.resolve(null),
    signOut: () => undefined,
  })
  const session = makeSession(opts.status ?? 'signedIn', opts.session)
  const tree = opts.routes ?? (
    <Routes>
      <Route path="*" element={ui} />
    </Routes>
  )
  // The app mounts the workspace provider only for a signed-in user; so does the harness.
  const signedIn = session.status === 'signedIn'
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
              {signedIn ? <WorkspaceScope>{tree}</WorkspaceScope> : tree}
            </MemoryRouter>
          </SessionContext.Provider>
        </ApiContext.Provider>
      </ConfigContext.Provider>
    </QueryClientProvider>,
  )
}
