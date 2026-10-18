import { Trans, useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { useFormat } from '@/i18n/useFormat'
import { isAuditKind, useKindLabel } from './kinds'
import type { AuditEvent, UserRef } from './queries'

function RefLink({ refValue, filter }: { refValue: UserRef | null | undefined; filter: 'actor' | 'subject' }) {
  const { t } = useTranslation('audit')
  if (!refValue) return <span>—</span>
  let label
  if (refValue.deleted) label = t('deletedUser')
  else if (refValue.display_name) label = <bdi dir="auto">{refValue.display_name}</bdi>
  else label = <bdi dir="ltr">{refValue.id.slice(0, 8)}</bdi>
  return (
    <Link className="no-underline hover:underline" to={`/audit?${filter}=${refValue.id}`}>
      {label}
    </Link>
  )
}

/** Audit timeline for the Audit page and Home; clicking a user filters the Audit page by them. */
export function AuditList({ events }: { events: AuditEvent[] }) {
  const { relative, dateTime } = useFormat()
  const kindLabel = useKindLabel()
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
                <span className="font-semibold" dir={isAuditKind(e.kind) ? undefined : 'ltr'}>
                  {kindLabel(e.kind)}
                </span>
                <span className="text-sm text-muted-foreground">
                  <Trans
                    i18nKey="by"
                    ns="audit"
                    components={{
                      actor: <RefLink refValue={e.actor} filter="actor" />,
                      subject: <RefLink refValue={e.subject} filter="subject" />,
                    }}
                  />
                </span>
                {isAuditKind(e.kind) && (
                  <code dir="ltr" className="font-mono text-xs text-faint">
                    {e.kind}
                  </code>
                )}
              </div>
              <time className="font-mono text-xs whitespace-nowrap text-faint" dateTime={e.at} title={dateTime(e.at)}>
                {relative(e.at)}
              </time>
            </div>
          </li>
        )
      })}
    </ol>
  )
}
