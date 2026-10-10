import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { vi } from 'vitest'
import { StartupError } from './StartupError'

it('shows the failure and re-runs startup on retry', async () => {
  const onRetry = vi.fn()
  render(<StartupError message="GET /config.json failed with 503" onRetry={onRetry} />)
  expect(screen.getByRole('heading', { name: /could not start/i })).toBeInTheDocument()
  expect(screen.getByRole('alert')).toHaveTextContent('failed with 503')
  await userEvent.setup().click(screen.getByRole('button', { name: /try again/i }))
  expect(onRetry).toHaveBeenCalledOnce()
})
