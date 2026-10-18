import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import arMe from '@/i18n/ar/me.json'
import { localeUnsaved, setLocale, setLocaleUnsaved } from '@/i18n'
import { server } from '@/test/server'
import { API } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { LoginPage } from './LoginPage'

afterEach(async () => {
  setLocaleUnsaved(false)
  await setLocale('en-US')
})

it('shows why the user is signed out', () => {
  renderApp(<LoginPage />, { status: 'signedOut', session: { notice: 'Sign-in was cancelled or denied.' } })
  expect(screen.getByRole('alert')).toHaveTextContent('Sign-in was cancelled or denied.')
  expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
})

it('language switch on the sign-in page turns the page Arabic', async () => {
  const saved = vi.fn()
  server.use(
    http.put(`${API}/api/v1/me/preferences`, () => {
      saved()
      return HttpResponse.json({ locale: 'ar' })
    }),
  )
  const user = userEvent.setup()
  renderApp(<LoginPage />, { status: 'signedOut' })
  await user.click(screen.getByRole('combobox', { name: 'Language' }))
  await user.click(await screen.findByRole('option', { name: 'العربية' }))
  expect(await screen.findByRole('heading', { name: arMe.login.title })).toBeInTheDocument()
  expect(document.documentElement.dir).toBe('rtl')
  expect(saved).not.toHaveBeenCalled()
  // Saved to the account after sign-in, by LocaleSync.
  expect(localeUnsaved()).toBe(true)
})
