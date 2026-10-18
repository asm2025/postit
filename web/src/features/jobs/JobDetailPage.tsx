import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { Panel, PanelTitle } from '@/components/Panel'
import { Skeleton } from '@/components/ui/skeleton'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { AttemptList } from './AttemptList'
import { JobActions } from './JobActions'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { attemptsText, summaryText } from './labels'
import { useJob } from './queries'

export function JobDetailPage() {
  const { t } = useTranslation('jobs')
  const { dateTime, number } = useFormat()
  const when = (iso?: string | null) => (iso ? dateTime(iso) : '—')
  const { id = '' } = useParams()
  const q = useJob(id)
  const d = q.data
  const job = d?.job
  const facts: [string, ReactNode][] = job
    ? [
        [
          t('detail.facts.id'),
          <span className="font-mono" dir="ltr">
            {job.id}
          </span>,
        ],
        [
          t('detail.facts.type'),
          <span className="font-mono" dir="ltr">
            {job.job_type}
          </span>,
        ],
        [
          t('detail.facts.queue'),
          <span dir="ltr" key="queue">
            {job.queue}
          </span>,
        ],
        [t('detail.facts.attempts'), attemptsText(job.attempts, job.max_attempts, number)],
        [t('detail.facts.created'), when(job.created_at)],
        [t('detail.facts.due'), when(job.run_at)],
        [t('detail.facts.started'), when(job.started_at)],
        [t('detail.facts.finished'), when(job.finished_at)],
        [
          t('detail.facts.schedule'),
          job.recurring_name ? (
            <span dir="ltr" key="schedule">
              {job.recurring_name}
            </span>
          ) : (
            '—'
          ),
        ],
        [
          t('detail.facts.summary'),
          <span dir="auto" key="summary">
            {summaryText(job.summary) || '—'}
          </span>,
        ],
      ]
    : []
  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">{t('detail.title')}</h1>
      <JobsNav />
      {q.isError && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(q.error)}
        </p>
      )}
      {!d && !q.isError && <Skeleton className="h-56" />}
      {d && job && (
        <>
          <Panel>
            <PanelTitle>{t('detail.summary')}</PanelTitle>
            <div className="flex items-center gap-3">
              <JobState state={job.state} />
            </div>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
              {facts.map(([k, v]) => (
                <div key={k} className="contents">
                  <dt className="text-muted-foreground">{k}</dt>
                  <dd className="min-w-0 break-words">{v}</dd>
                </div>
              ))}
            </dl>
            {(d.retried_from != null || d.retried_by != null) && (
              <div className="flex flex-wrap gap-4 text-sm">
                {d.retried_from && (
                  <Link className="underline" to={`/jobs/${d.retried_from}`}>
                    {t('detail.retryOf', { id: isolate(d.retried_from.slice(0, 8)) })}
                  </Link>
                )}
                {d.retried_by && (
                  <Link className="underline" to={`/jobs/${d.retried_by}`}>
                    {t('detail.retriedAs', { id: isolate(d.retried_by.slice(0, 8)) })}
                  </Link>
                )}
              </div>
            )}
            <JobActions job={job} />
          </Panel>
          <Panel>
            <PanelTitle>{t('detail.attemptsTitle')}</PanelTitle>
            <AttemptList attempts={d.attempts} />
          </Panel>
        </>
      )}
    </div>
  )
}
