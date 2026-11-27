import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { vi } from 'vitest'
import enAccounts from '@/i18n/en-US/accounts.json'
import { API, delegatedWorkspaceFixture, meFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { getActAs } from '@/workspace/store'
import { OAuthDonePage } from './OAuthDonePage'
import { OAUTH_RESULT_CODES } from './oauthCodes'
import { loadPending } from './pendingStore'

const ME = '00000000-0000-7000-8000-000000000001'
const OWNER = '0000000a-0000-7000-8000-00000000000a'
const FUTURE = '2999-01-01T00:10:00Z'

function app() {
  return (
    <Routes>
      <Route path="/oauth-done" element={<OAuthDonePage />} />
      <Route path="/accounts" element={<p>the accounts page</p>} />
    </Routes>
  )
}

function mock() {
  const cancel = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/accounts/connect/cid-9`, () =>
      HttpResponse.json({
        connect: 'oauth',
        discovered: [{ remote_id: 'r1', display_name: 'Chirp shop', handle: '@chirp' }],
        expires_at: FUTURE,
        pending_id: 'pend-9',
        platform: 'p2',
      }),
    ),
    http.delete(`${API}/api/v1/accounts/pending-connections/:id`, ({ params }) => {
      cancel(params.id)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return cancel
}

afterEach(() => {
  sessionStorage.clear()
  window.history.replaceState(null, '', '/')
})

it('oauth-done ok shows the checklist and strips the query', async () => {
  mock()
  window.history.replaceState(null, '', '/oauth-done?result=ok&connect_id=cid-9')
  renderApp(null, { routes: app(), route: '/oauth-done?result=ok&connect_id=cid-9' })

  expect(await screen.findByRole('checkbox', { name: /Chirp shop/ })).toBeChecked()
  expect(window.location.search).toBe('')
  expect(window.location.href).not.toContain('cid-9')
  // Kept for a reload inside its ten minutes, in this tab only.
  await waitFor(() => {
    expect(loadPending(ME)?.connectId).toBe('cid-9')
  })
  expect(loadPending(ME)?.pendingId).toBe('pend-9')
})

it('cancel on the oauth-done checklist discards the sign-in and goes back to the accounts', async () => {
  const cancel = mock()
  const user = userEvent.setup()
  renderApp(null, { routes: app(), route: '/oauth-done?result=ok&connect_id=cid-9' })
  await user.click(await screen.findByRole('button', { name: 'Cancel' }))
  await waitFor(() => {
    expect(cancel).toHaveBeenCalledWith('pend-9')
  })
  expect(await screen.findByText('the accounts page')).toBeInTheDocument()
})

it('a connection that expired says so', async () => {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/accounts/connect/cid-9`, () =>
      HttpResponse.json({ code: 'not_found', status: 404, title: 'x' }, { status: 404 }),
    ),
  )
  renderApp(null, { routes: app(), route: '/oauth-done?result=ok&connect_id=cid-9' })
  expect(await screen.findByText('This connection expired. Start again.')).toBeInTheDocument()
  expect(screen.getByRole('link', { name: 'Back to accounts' })).toHaveAttribute('href', '/accounts')
})

it('oauth-done error codes are translated (every code)', async () => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())))
  expect(OAUTH_RESULT_CODES.length).toBe(9)
  for (const code of OAUTH_RESULT_CODES) {
    const view = renderApp(null, { routes: app(), route: `/oauth-done?result=error&code=${code}` })
    expect(await screen.findByText(enAccounts.oauthResult[code])).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Back to accounts' })).toBeInTheDocument()
    view.unmount()
  }
})

it('an unknown code still says the sign-in did not finish', async () => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())))
  renderApp(null, { routes: app(), route: '/oauth-done?result=error&code=something_new' })
  expect(await screen.findByText(enAccounts.oauthResult.unknown)).toBeInTheDocument()
})

it('whoever started the connection finishes it: a delegated tab returns to its own workspace first', async () => {
  mock()
  renderApp(null, {
    routes: app(),
    route: '/oauth-done?result=ok&connect_id=cid-9',
    actAs: OWNER,
    workspaces: [ownWorkspaceFixture(), delegatedWorkspaceFixture(OWNER)],
  })
  expect(await screen.findByRole('checkbox', { name: /Chirp shop/ })).toBeInTheDocument()
  expect(getActAs()).toBeNull()
})
