import { useQueryClient } from '@tanstack/react-query'
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { Route, Routes, useLocation } from 'react-router'
import { useWorkspace } from '@/workspace/context'
import { API, delegatedWorkspaceFixture, meFixture, notificationFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { NotificationBell } from './NotificationBell'
import { useUnreadCount } from './queries'

const ME = '00000000-0000-7000-8000-000000000001'
const OWNER = '0000000a-0000-7000-8000-00000000000a'
const WORKSPACES = [ownWorkspaceFixture(), delegatedWorkspaceFixture(OWNER)]

function Where() {
  const { pathname, search } = useLocation()
  const { actAs } = useWorkspace()
  return (
    <p data-testid="where">
      {pathname}
      {search} in {actAs ?? 'own'}
    </p>
  )
}

function app(extra?: React.ReactNode) {
  return (
    <Routes>
      <Route
        path="*"
        element={
          <>
            <NotificationBell />
            <Where />
            {extra}
          </>
        }
      />
    </Routes>
  )
}

function mock(notices: ReturnType<typeof notificationFixture>[], unread = notices.filter((n) => !n.read_at).length) {
  const state = { unread, read: vi.fn() }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/notifications/unread-count`, () => HttpResponse.json({ count: state.unread })),
    http.get(`${API}/api/v1/notifications`, () =>
      HttpResponse.json({ data: notices, page: 1, page_size: 10, total: notices.length }),
    ),
    http.post(`${API}/api/v1/notifications/read`, async ({ request }) => {
      state.read(await request.json())
      state.unread = 0
      return HttpResponse.json({ updated: notices.length })
    }),
  )
  return state
}

it('bell shows unread badge and polls', async () => {
  mock([notificationFixture('account_needs_reconnect')], 3)
  function Options() {
    const qc = useQueryClient()
    useUnreadCount() // re-renders when the count arrives
    const options = qc.getQueryCache().find({ queryKey: ['notifications', 'unread'] })?.observers[0]?.options
    return (
      <p data-testid="options">
        {String(options?.refetchInterval)}|{String(options?.refetchIntervalInBackground)}|
        {String(options?.refetchOnWindowFocus)}
      </p>
    )
  }
  renderApp(null, { routes: app(<Options />) })
  expect(await screen.findByRole('button', { name: 'Notifications, 3 unread' })).toBeInTheDocument()
  expect(screen.getByText('3')).toBeInTheDocument()
  // Every minute while the tab is visible, and again when it regains focus.
  await waitFor(() => {
    expect(screen.getByTestId('options')).toHaveTextContent('60000|false|true')
  })
})

it('hides the badge at zero and caps it above 99', async () => {
  mock([], 0)
  const first = renderApp(null, { routes: app() })
  expect(await screen.findByRole('button', { name: 'Notifications' })).toBeInTheDocument()
  expect(screen.queryByText('0')).not.toBeInTheDocument()
  first.unmount()

  mock([], 150)
  renderApp(null, { routes: app() })
  expect(await screen.findByText('99+')).toBeInTheDocument()
})

it('mark all read clears the badge', async () => {
  const state = mock([notificationFixture('account_needs_reconnect')], 2)
  const user = userEvent.setup()
  renderApp(null, { routes: app() })
  await user.click(await screen.findByRole('button', { name: 'Notifications, 2 unread' }))
  await user.click(await screen.findByRole('button', { name: 'Mark all read' }))
  await waitFor(() => {
    expect(state.read).toHaveBeenCalledWith({ all: true })
  })
  expect(await screen.findByRole('button', { name: 'Notifications' })).toBeInTheDocument()
})

it('lists the newest notifications and links to all of them', async () => {
  mock([notificationFixture('account_needs_reconnect')])
  const user = userEvent.setup()
  renderApp(null, { routes: app() })
  await user.click(await screen.findByRole('button', { name: /Notifications/ }))
  const popover = await screen.findByRole('dialog')
  expect(within(popover).getByText('Reconnect an account')).toBeInTheDocument()
  expect(within(popover).getByText('Shop')).toBeInTheDocument()
  expect(within(popover).getByRole('link', { name: 'See all' })).toHaveAttribute('href', '/notifications')
})

it('opening an account notice from another workspace switches first', async () => {
  const state = mock([notificationFixture('account_needs_reconnect', { owner_id: OWNER })])
  const user = userEvent.setup()
  renderApp(null, { routes: app(), workspaces: WORKSPACES })
  await user.click(await screen.findByRole('button', { name: /Notifications/ }))
  await user.click(await screen.findByRole('button', { name: /Reconnect an account/ }))

  await waitFor(() => {
    expect(screen.getByTestId('where')).toHaveTextContent(`/accounts/018f0000-0000-7000-8000-0000000000ac in ${OWNER}`)
  })
  expect(state.read).toHaveBeenCalledWith({ ids: ['018f0000-0000-7000-8000-0000000000aa'] })
})

it('opening an account notice about my own workspace switches back to it', async () => {
  mock([notificationFixture('account_needs_reconnect', { owner_id: ME })])
  const user = userEvent.setup()
  renderApp(null, { routes: app(), actAs: OWNER, workspaces: WORKSPACES })
  await user.click(await screen.findByRole('button', { name: /Notifications/ }))
  await user.click(await screen.findByRole('button', { name: /Reconnect an account/ }))
  await waitFor(() => {
    expect(screen.getByTestId('where')).toHaveTextContent('in own')
  })
})

it('opening delegation_changed never switches workspace', async () => {
  // A delegate changed one of the user's grants: the notice names the user as owner.
  mock([notificationFixture('delegation_changed', { owner_id: ME })])
  const user = userEvent.setup()
  renderApp(null, { routes: app(), actAs: OWNER, workspaces: WORKSPACES })
  await user.click(await screen.findByRole('button', { name: /Notifications/ }))
  await user.click(await screen.findByRole('button', { name: /Shared access changed/ }))
  await waitFor(() => {
    expect(screen.getByTestId('where')).toHaveTextContent(`/delegations?tab=granted in ${OWNER}`)
  })
})

it('a notice with no page yet leaves the user on the list', async () => {
  mock([notificationFixture('delivery_failed')])
  const user = userEvent.setup()
  renderApp(null, { routes: app() })
  await user.click(await screen.findByRole('button', { name: /Notifications/ }))
  await user.click(await screen.findByRole('button', { name: /Post failed/ }))
  await waitFor(() => {
    expect(screen.getByTestId('where')).toHaveTextContent('/notifications in own')
  })
})
