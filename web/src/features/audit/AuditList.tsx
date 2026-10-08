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
    <Link className="no-underline hover:underline" to={`/audit?${filter}=${refValue.id}`}>
      {describeUserRef(refValue)}
    </Link>
  )
}

/** Audit timeline for the Audit page and Home; clicking a user filters the Audit page by them. */
export function AuditList({ events }: { events: AuditEvent[] }) {
  return (
    <ol className="flex flex-col">
      {events.map((e, i) => {
        const last = i === events.length - 1
        return (
          <li key={e.id} className="flex gap-4">
            <div aria-hidden="true" className="flex flex-col items-center pt-1.5">
              {i === 0 ? (
                <span className="size-2.5 rounded-full bg-pink shadow-[0_0_0_4px_var(--pink-soft)]" />
              ) : (
                <span className="box-border size-2.5 rounded-full border-2 border-faint" />
              )}
              {!last && <span className="mt-1.5 w-px flex-1 bg-line" />}
            </div>
            <div className={`flex min-w-0 flex-1 flex-wrap justify-between gap-x-4 gap-y-1 ${last ? '' : 'pb-[22px]'}`}>
              <div className="flex min-w-0 flex-col gap-0.5">
                <span className="font-semibold">{kindLabel(e.kind)}</span>
                <span className="text-sm text-muted-foreground">
                  by <RefLink refValue={e.actor} filter="actor" /> on <RefLink refValue={e.subject} filter="subject" />
                </span>
                {kindLabel(e.kind) !== e.kind && <code className="font-mono text-xs text-faint">{e.kind}</code>}
              </div>
              <time
                className="font-mono text-xs whitespace-nowrap text-faint"
                dateTime={e.at}
                title={new Date(e.at).toLocaleString()}
              >
                {relativeTime(e.at)}
              </time>
            </div>
          </li>
        )
      })}
    </ol>
  )
}
