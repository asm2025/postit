import { expect, it } from 'vitest'
import arJobs from '@/i18n/ar/jobs.json'
import enJobs from '@/i18n/en-US/jobs.json'
import { attemptsText, JOB_STATES, stateKey } from './labels'

it('shows attempts against the limit, or the count alone when there is none', () => {
  expect(attemptsText(1, 3)).toBe('1/3')
  expect(attemptsText(2, null)).toBe('2')
})

it('has a catalog key for every state in both languages', () => {
  expect(JOB_STATES.length).toBeGreaterThan(0)
  for (const state of JOB_STATES) {
    expect(stateKey(state)).toBe(`states.${state}`)
    expect(enJobs.states[state], `en-US ${state}`).toBeTruthy()
    expect(arJobs.states[state], `ar ${state}`).toBeTruthy()
  }
})

it('has no key for a state added later', () => {
  expect(stateKey('paused')).toBeNull()
})
