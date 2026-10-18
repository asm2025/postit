import { useTranslation } from 'react-i18next'
import type { JobState } from './queries'

/** One entry per generated state; a state added to the API fails typecheck here until listed. Labels live in the `jobs` catalog (`states.<state>`). */
const STATES: Record<JobState, true> = {
  queued: true,
  scheduled: true,
  running: true,
  succeeded: true,
  failed: true,
  dead: true,
  killed: true,
  cancelled: true,
}

/** Display order for state filters and charts. */
export const STATE_ORDER: JobState[] = [
  'queued',
  'scheduled',
  'running',
  'succeeded',
  'failed',
  'dead',
  'killed',
  'cancelled',
]

export const JOB_STATES = Object.keys(STATES) as JobState[]

export const isJobState = (v: string | null): v is JobState => v !== null && v in STATES

/** Catalog key for a state, or null for a state added later. */
export const stateKey = (state: string): `states.${JobState}` | null => (isJobState(state) ? `states.${state}` : null)

/**
 * Clients must tolerate states added later: an unknown state shows as the raw string.
 * Returns the label for a state in the screen language.
 */
export function useStateLabel(): (state: string) => string {
  const { t } = useTranslation('jobs')
  return (state) => (isJobState(state) ? t(`states.${state}`) : state)
}

/** Tries so far against the limit; a job with no limit (or none recorded yet) shows the count alone. `fmt` formats each number for the screen language. */
export const attemptsText = (attempts: number, max?: number | null, fmt: (n: number) => string = String) =>
  max == null ? fmt(attempts) : `${fmt(attempts)}/${fmt(max)}`

/** Payload summary as `key: value` pairs. */
export const summaryText = (summary: Record<string, string>) =>
  Object.entries(summary)
    .map(([k, v]) => `${k}: ${v}`)
    .join(', ')
