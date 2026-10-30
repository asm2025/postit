import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { vi } from 'vitest'
import { Switch } from './switch'

function Harness({ onChange }: { onChange?: (v: boolean) => void }) {
  const [on, setOn] = useState(false)
  return (
    <Switch
      aria-label="Email me"
      checked={on}
      onCheckedChange={(v) => {
        setOn(v)
        onChange?.(v)
      }}
    />
  )
}

it('is a labelled switch that toggles by click', async () => {
  const onChange = vi.fn()
  const user = userEvent.setup()
  render(<Harness onChange={onChange} />)
  const s = screen.getByRole('switch', { name: 'Email me' })
  expect(s).toHaveAttribute('aria-checked', 'false')
  await user.click(s)
  expect(s).toHaveAttribute('aria-checked', 'true')
  expect(onChange).toHaveBeenCalledWith(true)
})

it('toggles by keyboard (Space) and takes focus by Tab', async () => {
  const user = userEvent.setup()
  render(<Harness />)
  await user.tab()
  const s = screen.getByRole('switch', { name: 'Email me' })
  expect(s).toHaveFocus()
  await user.keyboard(' ')
  expect(s).toHaveAttribute('aria-checked', 'true')
})

it('does not toggle while disabled', async () => {
  const onCheckedChange = vi.fn()
  const user = userEvent.setup()
  render(<Switch aria-label="Off" disabled onCheckedChange={onCheckedChange} />)
  await user.click(screen.getByRole('switch', { name: 'Off' }))
  expect(onCheckedChange).not.toHaveBeenCalled()
})
