import { lazy, Suspense } from 'react'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { Panel, PanelTitle } from '@/components/Panel'
import { Skeleton } from '@/components/ui/skeleton'
import { JobsNav } from './JobsNav'
import { labelFor, STATE_ORDER } from './labels'
import { useJobStats } from './queries'

// recharts is large; keep it out of the main chunk.
const ThroughputChart = lazy(() => import('./ThroughputChart'))

export function JobsDashboard() {
  const stats = useJobStats()
  const data = stats.data
  const count = (key: string) => data?.states.find((s) => s.key === key)?.count ?? 0
  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">Jobs</h1>
      <JobsNav />
      {stats.isError && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(stats.error)}
        </p>
      )}
      <Panel>
        <PanelTitle>States</PanelTitle>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {STATE_ORDER.map((state) =>
            data ? (
              <Link
                key={state}
                to={`/jobs/list?state=${state}`}
                className="flex min-h-11 flex-col gap-1 rounded-xl border border-line bg-raised p-3 text-ink hover:border-pink"
              >
                <span className="font-display text-3xl font-bold">{count(state)}</span>
                <span className="font-mono text-xs text-faint">{labelFor(state)}</span>
              </Link>
            ) : (
              <Skeleton key={state} className="h-20 rounded-xl" />
            ),
          )}
        </div>
      </Panel>
      <Panel>
        <PanelTitle>Throughput, last {data?.window_hours ?? 24} hours</PanelTitle>
        {data ? (
          <Suspense fallback={<Skeleton className="h-56" />}>
            <ThroughputChart buckets={data.throughput} />
          </Suspense>
        ) : (
          <Skeleton className="h-56" />
        )}
      </Panel>
      <Panel>
        <PanelTitle>Queue depth</PanelTitle>
        {data ? (
          data.queues.length === 0 ? (
            <p className="text-sm text-faint">No queued work.</p>
          ) : (
            <ul className="space-y-1 font-mono text-sm">
              {data.queues.map((q) => (
                <li key={q.key} className="flex justify-between gap-4">
                  <span>{q.key}</span>
                  <span>{q.count}</span>
                </li>
              ))}
            </ul>
          )
        ) : (
          <Skeleton className="h-10" />
        )}
      </Panel>
    </div>
  )
}
