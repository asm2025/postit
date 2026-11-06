import { screen } from '@testing-library/react'
import { localeUnsaved, setLocale, setLocaleUnsaved } from '@/i18n'
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

it('shows no language switch while one language ships', () => {
  renderApp(<LoginPage />, { status: 'signedOut' })
  expect(screen.queryByRole('combobox', { name: 'Language' })).not.toBeInTheDocument()
  expect(localeUnsaved()).toBe(false)
})
