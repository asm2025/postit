import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { vi } from 'vitest'
import enCommon from '@/i18n/en-US/common.json'
import type { StartupReason } from '@/config/runtime'
import { StartupError } from './StartupError'

it('shows the failure and re-runs startup on retry', async () => {
  const onRetry = vi.fn()
  render(<StartupError reason="runtime_config" onRetry={onRetry} />)
  expect(screen.getByRole('heading', { name: /could not start/i })).toBeInTheDocument()
  await userEvent.setup().click(screen.getByRole('button', { name: /try again/i }))
  expect(onRetry).toHaveBeenCalledOnce()
})

it.each(['runtime_config', 'auth_config', 'identity_unreachable'] as StartupReason[])(
  'startup error shows the translated reason for %s',
  (reason) => {
    render(<StartupError reason={reason} onRetry={() => undefined} />)
    expect(screen.getByRole('alert')).toHaveTextContent(enCommon.startup.reason[reason])
  },
)

it('says something generic when the failure has no reason', () => {
  render(<StartupError onRetry={() => undefined} />)
  expect(screen.getByRole('alert')).toHaveTextContent(enCommon.startup.reason.unknown)
})
