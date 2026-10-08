import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, vi } from 'vitest'
import { ThemeProvider } from './ThemeProvider'
import { ThemeToggle } from './ThemeToggle'

function stubMatchMedia(dark: boolean) {
  vi.stubGlobal('matchMedia', () => ({
    matches: dark,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }))
}

function setup() {
  return render(
    <ThemeProvider>
      <ThemeToggle />
    </ThemeProvider>,
  )
}

beforeEach(() => {
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  stubMatchMedia(false)
})
afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

it('defaults to system and marks it pressed', () => {
  setup()
  expect(screen.getByRole('button', { name: 'System theme' })).toHaveAttribute('aria-pressed', 'true')
  expect(screen.getByRole('button', { name: 'Light theme' })).toHaveAttribute('aria-pressed', 'false')
})

it('switches theme, stores the choice and toggles the dark class', async () => {
  const user = userEvent.setup()
  setup()
  await user.click(screen.getByRole('button', { name: 'Dark theme' }))
  expect(document.documentElement).toHaveClass('dark')
  expect(localStorage.getItem('postit.theme')).toBe('dark')
  expect(screen.getByRole('button', { name: 'Dark theme' })).toHaveAttribute('aria-pressed', 'true')
  await user.click(screen.getByRole('button', { name: 'Light theme' }))
  expect(document.documentElement).not.toHaveClass('dark')
})

it('restores the stored choice on mount', () => {
  localStorage.setItem('postit.theme', 'dark')
  setup()
  expect(screen.getByRole('button', { name: 'Dark theme' })).toHaveAttribute('aria-pressed', 'true')
  expect(document.documentElement).toHaveClass('dark')
})

it('treats an invalid stored value as system', () => {
  localStorage.setItem('postit.theme', 'purple')
  setup()
  expect(screen.getByRole('button', { name: 'System theme' })).toHaveAttribute('aria-pressed', 'true')
})

it('follows the OS when set to system', () => {
  stubMatchMedia(true)
  setup()
  expect(document.documentElement).toHaveClass('dark')
})

it('still works when storage is blocked', async () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
    throw new Error('blocked')
  })
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
    throw new Error('blocked')
  })
  const user = userEvent.setup()
  setup()
  expect(screen.getByRole('button', { name: 'System theme' })).toHaveAttribute('aria-pressed', 'true')
  await user.click(screen.getByRole('button', { name: 'Dark theme' }))
  expect(document.documentElement).toHaveClass('dark')
  expect(screen.getByRole('button', { name: 'Dark theme' })).toHaveAttribute('aria-pressed', 'true')
})
