import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import enMe from '@/i18n/en-US/me.json'
import { server } from '@/test/server'
import { API, meFixture, preferencesFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { ProfilePage } from './ProfilePage'

const me = meFixture({ display_name: 'Ada Lovelace', email: 'ada@postit.com', account_url: 'https://idp.test/account' })

beforeEach(() => {
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(me)),
    http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json(preferencesFixture())),
  )
})

it('shows IdP-owned fields read-only with an account link', async () => {
  renderApp(<ProfilePage />)
  expect(await screen.findByText('Ada Lovelace')).toBeInTheDocument()
  expect(screen.getByText('ada@postit.com')).toBeInTheDocument()
  expect(screen.getByRole('link', { name: /manage account/i })).toHaveAttribute('href', 'https://idp.test/account')
})

it('requires typing the display name before deleting the account', async () => {
  const deleted = vi.fn()
  server.use(
    http.delete(`${API}/api/v1/me`, async ({ request }) => {
      deleted(await request.json())
      return new HttpResponse(null, { status: 202 })
    }),
  )
  const user = userEvent.setup()
  renderApp(<ProfilePage />)
  await user.click(await screen.findByRole('button', { name: /delete my postit account/i }))
  const confirm = screen.getByRole('button', { name: /delete account/i })
  expect(confirm).toBeDisabled()
  await user.type(screen.getByLabelText(/type your name/i), 'Ada Lovelace')
  expect(confirm).toBeEnabled()
  await user.click(confirm)
  await waitFor(() => {
    expect(deleted).toHaveBeenCalledWith({ display_name: 'Ada Lovelace' })
  })
})

it('shows the server message when deletion is refused', async () => {
  server.use(
    http.delete(`${API}/api/v1/me`, () =>
      HttpResponse.json({ code: 'last_admin', status: 409, title: 'x' }, { status: 409 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(<ProfilePage />)
  await user.click(await screen.findByRole('button', { name: /delete my postit account/i }))
  await user.type(screen.getByLabelText(/type your name/i), 'Ada Lovelace')
  await user.click(screen.getByRole('button', { name: /delete account/i }))
  expect(await screen.findByText(/last active admin/i)).toBeInTheDocument()
})

it('shows no language row while one language ships', async () => {
  renderApp(<ProfilePage />)
  await screen.findByText('Ada Lovelace')
  expect(screen.queryByText(enMe.profile.language)).not.toBeInTheDocument()
  expect(screen.queryByRole('combobox', { name: 'Language' })).not.toBeInTheDocument()
})
it('display name uses automatic direction', async () => {
  renderApp(<ProfilePage />)
  expect(await screen.findByText('Ada Lovelace')).toHaveAttribute('dir', 'auto')
})

it('shows the notification settings', async () => {
  renderApp(<ProfilePage />)
  expect(await screen.findByRole('heading', { name: enMe.notifications.title })).toBeInTheDocument()
  expect(await screen.findByRole('switch', { name: enMe.notifications.email.label })).toBeInTheDocument()
})

describe('time zone', () => {
  const browserIn = (zone: string) =>
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      timeZone: zone,
    } as Intl.ResolvedDateTimeFormatOptions)

  function mockPreferences(timezone: string) {
    const updates = vi.fn()
    server.use(
      http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json(preferencesFixture({ timezone }))),
      http.put(`${API}/api/v1/me/preferences`, async ({ request }) => {
        const body = (await request.json()) as Record<string, unknown>
        updates(body)
        return HttpResponse.json({ ...preferencesFixture({ timezone }), ...body })
      }),
    )
    return updates
  }

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('is titled for preferences too', async () => {
    renderApp(<ProfilePage />)
    expect(await screen.findByRole('heading', { name: 'Profile and preferences', level: 1 })).toBeInTheDocument()
  })

  it('time zone saves the chosen zone', async () => {
    browserIn('Europe/Paris')
    const updates = mockPreferences('Europe/Paris')
    const user = userEvent.setup()
    renderApp(<ProfilePage />)
    await user.click(await screen.findByRole('combobox', { name: 'Time zone' }))
    await user.click(await screen.findByRole('option', { name: 'Asia/Tokyo' }))
    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ timezone: 'Asia/Tokyo' })
    })
  })

  it('prompt offers the browser zone once', async () => {
    browserIn('Asia/Riyadh')
    const updates = mockPreferences('Europe/Paris')
    const user = userEvent.setup()
    const first = renderApp(<ProfilePage />)
    const group = await screen.findByRole('group', { name: 'Time zone question' })
    expect(group).toHaveTextContent('Asia/Riyadh')
    expect(group).toHaveTextContent('Europe/Paris')

    await user.click(within(group).getByRole('button', { name: 'Use Asia/Riyadh' }))

    await waitFor(() => {
      expect(updates).toHaveBeenCalledWith({ timezone: 'Asia/Riyadh' })
    })
    expect(screen.queryByRole('group', { name: 'Time zone question' })).not.toBeInTheDocument()
    expect(localStorage.getItem('postit.tzAsked')).toBe('1')
    // Asked once: a new visit does not ask again.
    first.unmount()
    renderApp(<ProfilePage />)
    await screen.findByRole('combobox', { name: 'Time zone' })
    expect(screen.queryByRole('group', { name: 'Time zone question' })).not.toBeInTheDocument()
  })

  it('keeping the saved zone also ends the question and changes nothing', async () => {
    browserIn('Asia/Riyadh')
    const updates = mockPreferences('Europe/Paris')
    const user = userEvent.setup()
    renderApp(<ProfilePage />)
    await user.click(await screen.findByRole('button', { name: 'Keep Europe/Paris' }))
    expect(screen.queryByRole('group', { name: 'Time zone question' })).not.toBeInTheDocument()
    expect(updates).not.toHaveBeenCalled()
    expect(localStorage.getItem('postit.tzAsked')).toBe('1')
  })

  it('does not ask when the browser matches the saved zone', async () => {
    browserIn('Europe/Paris')
    mockPreferences('Europe/Paris')
    renderApp(<ProfilePage />)
    await screen.findByRole('combobox', { name: 'Time zone' })
    expect(screen.queryByRole('group', { name: 'Time zone question' })).not.toBeInTheDocument()
  })
})
