import { expect, it } from 'vitest'
import { attemptsText, labelFor } from './labels'

it('shows attempts against the limit, or the count alone when there is none', () => {
  expect(attemptsText(1, 3)).toBe('1/3')
  expect(attemptsText(2, null)).toBe('2')
})

it('shows a state added later as its raw name', () => {
  expect(labelFor('paused')).toBe('paused')
})
