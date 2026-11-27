import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { vi } from 'vitest'
import { Shell } from '@/components/Shell'
import { Gate, RequireAdmin, RequireSession } from '@/features/me/Gate'
import { API, meFixture, platformFixture, userFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { UsersPage } from '@/features/users/UsersPage'
import { PlatformsPage } from './PlatformsPage'
import { PublishingPage } from './PublishingPage'
import type { AdminPlatform } from './queries'

const REDIRECT = 'https://api.postit.test/api/v1/platforms/p2/oauth/callback'
const rows: AdminPlatform[] = [
  { id: 'p1', name: 'Pixelfed', compiled_in: true, configured: true, enabled: true },
  { id: 'p2', name: 'Chirper', compiled_in: true, configured: false, enabled: false, redirect_uri: REDIRECT },
]

function admin(extra: () => void = () => undefined) {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'admin' }))),
    http.get(`${API}/api/v1/platforms`, () => HttpResponse.json([platformFixture('p1', 'Pixelfed')])),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
  )
  extra()
}

describe('platforms', () => {
  it('lists every platform with what it needs', async () => {
    admin(() => {
      server.use(http.get(`${API}/api/v1/admin/platforms`, () => HttpResponse.json(rows)))
    })
    renderApp(<PlatformsPage />)
    const table = await screen.findByRole('table', { name: 'Platforms' })
    const body = within(table).getAllByRole('row').slice(1)
    expect(within(body[0]).getByText('Pixelfed')).toBeInTheDocument()
    expect(within(body[1]).getByText('Chirper')).toBeInTheDocument()
    // The redirect URI shows only where there is one.
    expect(within(body[0]).queryByText(REDIRECT)).not.toBeInTheDocument()
    expect(within(body[1]).getByText(REDIRECT)).toBeInTheDocument()
  })

  it('enable toggle calls PUT', async () => {
    const put = vi.fn()
    admin(() => {
      server.use(
        http.get(`${API}/api/v1/admin/platforms`, () => HttpResponse.json(rows)),
        http.put(`${API}/api/v1/admin/platforms/:id`, async ({ params, request }) => {
          put(params.id, await request.json())
          return HttpResponse.json({ ...rows[1], enabled: true })
        }),
      )
    })
    const user = userEvent.setup()
    renderApp(<PlatformsPage />)
    await user.click(await screen.findByRole('switch', { name: 'Offer Chirper to people' }))
    await waitFor(() => {
      expect(put).toHaveBeenCalledWith('p2', { enabled: true })
    })
  })

  it('redirect URI copies', async () => {
    admin(() => {
      server.use(http.get(`${API}/api/v1/admin/platforms`, () => HttpResponse.json(rows)))
    })
    const user = userEvent.setup()
    // After setup, which installs its own clipboard.
    const writeText = vi.spyOn(navigator.clipboard, 'writeText')
    renderApp(<PlatformsPage />)
    await user.click(await screen.findByRole('button', { name: 'Copy the redirect URI of Chirper' }))
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(REDIRECT)
    })
    expect(await screen.findByText('Copied')).toBeInTheDocument()
  })
})

describe('publishing', () => {
  function settings(live: boolean, native: number) {
    const put = vi.fn()
    const state = { live, native_scheduled_count: native }
    admin(() => {
      server.use(
        http.get(`${API}/api/v1/admin/settings/publishing`, () => HttpResponse.json(state)),
        http.put(`${API}/api/v1/admin/settings/publishing`, async ({ request }) => {
          const body = (await request.json()) as { live: boolean }
          put(body)
          state.live = body.live
          return HttpResponse.json(state)
        }),
      )
    })
    return put
  }

  it('turning live off asks first and shows the native count', async () => {
    const put = settings(true, 4)
    const user = userEvent.setup()
    renderApp(<PublishingPage />)
    await user.click(await screen.findByRole('switch', { name: /Live publishing/ }))
    const confirm = await screen.findByRole('alertdialog')
    expect(
      within(confirm).getByText('4 posts are already scheduled on the platforms themselves and will still go out.'),
    ).toBeInTheDocument()
    expect(put).not.toHaveBeenCalled()
    await user.click(within(confirm).getByRole('button', { name: 'Turn off' }))
    await waitFor(() => {
      expect(put).toHaveBeenCalledWith({ live: false })
    })
  })

  it('declining leaves live on', async () => {
    const put = settings(true, 0)
    const user = userEvent.setup()
    renderApp(<PublishingPage />)
    await user.click(await screen.findByRole('switch', { name: /Live publishing/ }))
    const confirm = await screen.findByRole('alertdialog')
    expect(within(confirm).queryByText(/already scheduled/)).not.toBeInTheDocument()
    await user.click(within(confirm).getByRole('button', { name: 'Cancel' }))
    expect(put).not.toHaveBeenCalled()
    expect(screen.getByRole('switch', { name: /Live publishing/ })).toBeChecked()
  })

  it('turning live on needs no question', async () => {
    const put = settings(false, 2)
    const user = userEvent.setup()
    renderApp(<PublishingPage />)
    await user.click(await screen.findByRole('switch', { name: /Live publishing/ }))
    await waitFor(() => {
      expect(put).toHaveBeenCalledWith({ live: true })
    })
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })
})

describe('user limits', () => {
  const target = userFixture({
    id: '22222222-2222-7222-8222-222222222222',
    display_name: 'Ann Active',
    email: 'ann@x.com',
  })

  it('limits dialog saves all three fields', async () => {
    const put = vi.fn()
    admin(() => {
      server.use(
        http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [target], page: 1, page_size: 20, total: 1 })),
        http.get(`${API}/api/v1/admin/users/${target.id}/limits`, () =>
          HttpResponse.json({
            contacts_cap: 100,
            email_daily_cap: 50,
            storage_quota_bytes: 1000,
            updated_at: '2026-11-01T10:00:00Z',
          }),
        ),
        http.put(`${API}/api/v1/admin/users/${target.id}/limits`, async ({ request }) => {
          put(await request.json())
          return HttpResponse.json({
            contacts_cap: 200,
            email_daily_cap: 0,
            storage_quota_bytes: 5000,
            updated_at: '2026-11-02T10:00:00Z',
          })
        }),
      )
    })
    const user = userEvent.setup()
    renderApp(<UsersPage />)
    await user.click(await screen.findByRole('button', { name: /Actions for/ }))
    await user.click(await screen.findByRole('menuitem', { name: 'Limits' }))
    const dialog = await screen.findByRole('dialog')
    const contacts = await within(dialog).findByLabelText('Contacts')
    expect(contacts).toHaveValue(100)
    await user.clear(contacts)
    await user.type(contacts, '200')
    const emails = within(dialog).getByLabelText('Emails per day')
    await user.clear(emails)
    await user.type(emails, '0')
    const storage = within(dialog).getByLabelText('Media storage (bytes)')
    await user.clear(storage)
    await user.type(storage, '5000')
    await user.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(put).toHaveBeenCalledWith({ contacts_cap: 200, email_daily_cap: 0, storage_quota_bytes: 5000 })
    })
  })

  it('refuses a limit that is not a whole number', async () => {
    admin(() => {
      server.use(
        http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [target], page: 1, page_size: 20, total: 1 })),
        http.get(`${API}/api/v1/admin/users/${target.id}/limits`, () =>
          HttpResponse.json({
            contacts_cap: 1,
            email_daily_cap: 1,
            storage_quota_bytes: 1,
            updated_at: '2026-11-01T10:00:00Z',
          }),
        ),
      )
    })
    const user = userEvent.setup()
    renderApp(<UsersPage />)
    await user.click(await screen.findByRole('button', { name: /Actions for/ }))
    await user.click(await screen.findByRole('menuitem', { name: 'Limits' }))
    const dialog = await screen.findByRole('dialog')
    const contacts = await within(dialog).findByLabelText('Contacts')
    await user.clear(contacts)
    expect(within(dialog).getByRole('button', { name: 'Save' })).toBeDisabled()
  })
})

describe('who can reach them', () => {
  function app() {
    return (
      <Routes>
        <Route element={<RequireSession />}>
          <Route element={<Gate />}>
            <Route element={<Shell />}>
              <Route element={<RequireAdmin />}>
                <Route path="admin/platforms" element={<PlatformsPage />} />
                <Route path="admin/publishing" element={<PublishingPage />} />
              </Route>
            </Route>
          </Route>
        </Route>
      </Routes>
    )
  }

  it('member cannot reach the admin pages', async () => {
    server.use(
      http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture({ role: 'member' }))),
      http.get(`${API}/api/v1/admin/platforms`, () => HttpResponse.json(rows)),
    )
    renderApp(null, { routes: app(), route: '/admin/platforms' })
    expect(await screen.findByRole('heading', { name: /not allowed|forbidden|admin/i })).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Platforms' })).not.toBeInTheDocument()
  })

  it('an admin reaches them', async () => {
    admin(() => {
      server.use(
        http.get(`${API}/api/v1/admin/platforms`, () => HttpResponse.json(rows)),
        http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [], page: 1, page_size: 1, total: 0 })),
        http.get(`${API}/api/v1/admin/jobs/stats`, () =>
          HttpResponse.json({ states: [], queues: [], types: [], throughput: [], window_hours: 24 }),
        ),
      )
    })
    renderApp(null, { routes: app(), route: '/admin/platforms' })
    expect(await screen.findByRole('table', { name: 'Platforms' })).toBeInTheDocument()
  })
})
