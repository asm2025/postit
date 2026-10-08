import { Link } from 'react-router'
import { relativeTime } from '@/lib/format'
import { kindLabel } from './kinds'
import type { AuditEvent, UserRef } from './queries'

export function describeUserRef(ref: UserRef | null | undefined): string {
  if (!ref) return '—'
  if (ref.deleted) return 'deleted user'
  return ref.display_name ?? ref.id.slice(0, 8)
}

function RefLink({ refValue, filter }: { refValue: UserRef | null | undefined; filter: 'actor' | 'subject' }) {
  if (!refValue) return <span>—</span>
  return (
    <Link className="underline-offset-2 hover:underline" to={`/audit?${filter}=${refValue.id}`}>
      {describeUserRef(refValue)}
    </Link>
  )
}

/** Audit rows for the Audit page and Home; clicking a user filters the Audit page by them. */
export function AuditList({ events }: { events: AuditEvent[] }) {
  return (
    <ul className="divide-y text-sm">
      {events.map((e) => (
        <li key={e.id} className="flex flex-wrap items-baseline justify-between gap-2 py-2">
          <span>
            <span className="font-medium">{kindLabel(e.kind)}</span>
            <span className="text-muted-foreground">
              {' '}
              by <RefLink refValue={e.actor} filter="actor" /> on <RefLink refValue={e.subject} filter="subject" />
            </span>
          </span>
          <time className="text-muted-foreground" dateTime={e.at} title={new Date(e.at).toLocaleString()}>
            {relativeTime(e.at)}
          </time>
        </li>
      ))}
    </ul>
  )
}
