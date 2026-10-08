import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { ProfilePage } from './ProfilePage'

const me = meFixture({ display_name: 'Ada Lovelace', email: 'ada@postit.com', account_url: 'https://idp.test/account' })

beforeEach(() => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(me)))
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
