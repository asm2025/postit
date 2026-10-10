import type { ReactNode } from 'react'
import { Link, useParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { Panel, PanelTitle } from '@/components/Panel'
import { Skeleton } from '@/components/ui/skeleton'
import { AttemptList } from './AttemptList'
import { JobActions } from './JobActions'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { summaryText } from './labels'
import { useJob } from './queries'

const when = (iso?: string | null) => (iso ? new Date(iso).toLocaleString() : '—')

export function JobDetailPage() {
  const { id = '' } = useParams()
  const q = useJob(id)
  const d = q.data
  const job = d?.job
  const facts: [string, ReactNode][] = job
    ? [
        ['Job ID', <span className="font-mono">{job.id}</span>],
        ['Type', <span className="font-mono">{job.job_type}</span>],
        ['Queue', job.queue],
        ['Attempts', `${String(job.attempts)}/${String(job.max_attempts ?? '?')}`],
        ['Created', when(job.created_at)],
        ['Due', when(job.run_at)],
        ['Started', when(job.started_at)],
        ['Finished', when(job.finished_at)],
        ['Schedule', job.recurring_name ?? '—'],
        ['Summary', summaryText(job.summary) || '—'],
      ]
    : []
  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">Job</h1>
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
            <PanelTitle>Summary</PanelTitle>
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
                    Retry of {d.retried_from.slice(0, 8)}
                  </Link>
                )}
                {d.retried_by && (
                  <Link className="underline" to={`/jobs/${d.retried_by}`}>
                    Retried as {d.retried_by.slice(0, 8)}
                  </Link>
                )}
              </div>
            )}
            <JobActions job={job} />
          </Panel>
          <Panel>
            <PanelTitle>Attempts</PanelTitle>
            <AttemptList attempts={d.attempts} />
          </Panel>
        </>
      )}
    </div>
  )
}
