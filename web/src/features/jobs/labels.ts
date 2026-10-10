import type { JobState } from './queries'

/** One label per generated state; a state added to the API fails typecheck here until labelled. */
export const STATE_LABELS: Record<JobState, string> = {
  queued: 'Queued',
  scheduled: 'Scheduled',
  running: 'Running',
  succeeded: 'Succeeded',
  failed: 'Failed',
  dead: 'Dead',
  killed: 'Killed',
  cancelled: 'Cancelled',
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

/** Clients must tolerate states added later: unknown values show as sent. */
export function labelFor(state: string): string {
  return (STATE_LABELS as Record<string, string | undefined>)[state] ?? state
}

/** Payload summary as `key: value` pairs. */
export const summaryText = (summary: Record<string, string>) =>
  Object.entries(summary)
    .map(([k, v]) => `${k}: ${v}`)
    .join(', ')
