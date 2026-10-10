import { Badge } from '@/components/ui/badge'
import { labelFor } from './labels'

const WARN = new Set(['failed', 'dead', 'killed'])

/** A job state as a coloured badge. */
export function JobState({ state }: { state: string }) {
  const tone = WARN.has(state)
    ? 'bg-warn-bg text-warn-text'
    : state === 'running'
      ? 'bg-pink-soft text-pink-text'
      : 'bg-muted text-muted-foreground'
  return <Badge className={tone}>{labelFor(state)}</Badge>
}
