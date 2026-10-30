import { useQuery } from '@tanstack/react-query'
import { screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import userEvent from '@testing-library/user-event'
import { afterEach, onTestFinished, vi } from 'vitest'
import { setLocale } from '@/i18n'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { STORE_KEY, stored, stubPush } from '@/test/push'
import { renderApp } from '@/test/render'
import { Gate, RequireAdmin, RequireSession } from './Gate'

/** Stands in for an admin page: it calls an admin endpoint. */
function UsersProbe() {
  const api = useApi()
  useQuery({ queryKey: ['users', 'probe'], queryFn: () => unwrap(api.GET('/api/v1/users')) })
  return <div>users page</div>
}

function app() {
  return (
    <Routes>
      <Route path="/login" element={<div>login page</div>} />
      <Route element={<RequireSession />}>
        <Route element={<Gate />}>
          <Route index element={<div>dashboard</div>} />
          <Route element={<RequireAdmin />}>
            <Route path="users" element={<UsersProbe />} />
          </Route>
        </Route>
      </Route>
    </Routes>
  )
}

const problem = (code: string, status: number) => HttpResponse.json({ code, status, title: 'x' }, { status })
const me = (body: object, status = 200) => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(body, { status })))
}

it('sends signed-out visitors to the login page', async () => {
  renderApp(null, { status: 'signedOut', routes: app() })
  expect(await screen.findByText('login page')).toBeInTheDocument()
})

it('shows the pending screen for a pending user', async () => {
  me(meFixture({ status: 'pending' }))
  renderApp(null, { routes: app() })
  expect(await screen.findByText(/your account is waiting for approval/i)).toBeInTheDocument()
})

it('shows the unavailable screen when /me answers account_disabled, without refetch loops', async () => {
  const calls = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me`, () => {
      calls()
      return problem('account_disabled', 403)
    }),
  )
  renderApp(null, { routes: app() })
  expect(await screen.findByText(/account unavailable/i)).toBeInTheDocument()
  await new Promise((r) => setTimeout(r, 50))
  expect(calls).toHaveBeenCalledTimes(1)
})

it('lets an active member in', async () => {
  me(meFixture({ status: 'active', role: 'member' }))
  renderApp(null, { routes: app(), route: '/' })
  expect(await screen.findByText('dashboard')).toBeInTheDocument()
})

it('ends a push subscription another account registered on this browser', async () => {
  const env = stubPush({ permission: 'granted', existing: true })
  onTestFinished(env.restore)
  localStorage.setItem(
    STORE_KEY,
    stored('018f0000-0000-7000-8000-0000000000e1', '00000000-0000-7000-8000-000000000099'),
  )
  me(meFixture({ status: 'active' }))
  renderApp(null, { routes: app(), route: '/' })
  expect(await screen.findByText('dashboard')).toBeInTheDocument()
  await waitFor(() => {
    expect(env.subscription.unsubscribe).toHaveBeenCalled()
  })
  expect(JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')).toEqual({})
})

it('keeps this account push subscription', async () => {
  const env = stubPush({ permission: 'granted', existing: true })
  onTestFinished(env.restore)
  localStorage.setItem(STORE_KEY, stored('018f0000-0000-7000-8000-0000000000e1'))
  me(meFixture({ status: 'active' }))
  renderApp(null, { routes: app(), route: '/' })
  expect(await screen.findByText('dashboard')).toBeInTheDocument()
  await waitFor(() => {
    expect(env.getSubscription).toHaveBeenCalled()
  })
  expect(env.subscription.unsubscribe).not.toHaveBeenCalled()
  expect(localStorage.getItem(STORE_KEY)).toBe(stored('018f0000-0000-7000-8000-0000000000e1'))
})

it('forbids members on admin routes', async () => {
  me(meFixture({ status: 'active', role: 'member' }))
  renderApp(null, { routes: app(), route: '/users' })
  expect(await screen.findByRole('heading', { name: /not allowed/i })).toBeInTheDocument()
})

it('admits admins to admin routes', async () => {
  me(meFixture({ status: 'active', role: 'admin' }))
  server.use(http.get(`${API}/api/v1/users`, () => HttpResponse.json({ data: [], page: 1, page_size: 20, total: 0 })))
  renderApp(null, { routes: app(), route: '/users' })
  expect(await screen.findByText('users page')).toBeInTheDocument()
})

it('re-checks /me when a call answers account_disabled mid-session', async () => {
  let disabled = false
  server.use(
    http.get(`${API}/api/v1/me`, () =>
      disabled ? problem('account_disabled', 403) : HttpResponse.json(meFixture({ status: 'active', role: 'admin' })),
    ),
    http.get(`${API}/api/v1/users`, () => {
      disabled = true
      return problem('account_disabled', 403)
    }),
  )
  renderApp(null, { routes: app(), route: '/users' })
  expect(await screen.findByText(/account unavailable/i)).toBeInTheDocument()
})

it('re-checks /me on forbidden, so a demoted admin loses admin pages', async () => {
  let demoted = false
  server.use(
    http.get(`${API}/api/v1/me`, () =>
      HttpResponse.json(meFixture({ status: 'active', role: demoted ? 'member' : 'admin' })),
    ),
    http.get(`${API}/api/v1/users`, () => {
      demoted = true
      return problem('forbidden', 403)
    }),
  )
  renderApp(null, { routes: app(), route: '/users' })
  await waitFor(() => {
    expect(screen.getByRole('heading', { name: /not allowed/i })).toBeInTheDocument()
  })
})

describe('language switch on status screens', () => {
  afterEach(async () => {
    await setLocale('en-US')
  })

  async function chooseArabic() {
    const user = userEvent.setup()
    await user.click(await screen.findByRole('combobox', { name: 'Language' }))
    await user.click(await screen.findByRole('option', { name: 'العربية' }))
  }

  it('stays local on the disabled screen: it makes no preferences request', async () => {
    const put = vi.fn()
    server.use(
      http.get(`${API}/api/v1/me`, () => problem('account_disabled', 403)),
      http.put(`${API}/api/v1/me/preferences`, () => {
        put()
        return problem('account_disabled', 403)
      }),
    )
    renderApp(null, { routes: app() })
    await chooseArabic()
    await waitFor(() => {
      expect(document.documentElement.dir).toBe('rtl')
    })
    await new Promise((r) => setTimeout(r, 50))
    expect(put).not.toHaveBeenCalled()
    expect(document.documentElement.dir).toBe('rtl')
  })

  it('saves the choice from the pending screen', async () => {
    const body = vi.fn()
    me(meFixture({ status: 'pending' }))
    server.use(
      http.put(`${API}/api/v1/me/preferences`, async ({ request }) => {
        body(await request.json())
        return HttpResponse.json({ locale: 'ar' })
      }),
    )
    renderApp(null, { routes: app() })
    await chooseArabic()
    await waitFor(() => {
      expect(body).toHaveBeenCalledWith({ locale: 'ar' })
    })
    expect(document.documentElement.dir).toBe('rtl')
  })
})
