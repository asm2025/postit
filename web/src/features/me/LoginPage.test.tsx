import { screen } from '@testing-library/react'
import { renderApp } from '@/test/render'
import { LoginPage } from './LoginPage'

it('shows why the user is signed out', () => {
  renderApp(<LoginPage />, { status: 'signedOut', session: { notice: 'Sign-in was cancelled or denied.' } })
  expect(screen.getByRole('alert')).toHaveTextContent('Sign-in was cancelled or denied.')
  expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled()
})
