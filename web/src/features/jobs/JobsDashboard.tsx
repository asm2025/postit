import { lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { Panel, PanelTitle } from '@/components/Panel'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { Skeleton } from '@/components/ui/skeleton'
import { useFormat } from '@/i18n/useFormat'
import { JobsNav } from './JobsNav'
import { STATE_ORDER, useStateLabel } from './labels'
import { useJobStats } from './queries'

// recharts is large; keep it out of the main chunk.
const ThroughputChart = lazy(() => import('./ThroughputChart'))

export function JobsDashboard() {
  const { t } = useTranslation('jobs')
  const { number } = useFormat()
  const label = useStateLabel()
  const stats = useJobStats()
  const data = stats.data
  const hours = data?.window_hours ?? 24
  const count = (key: string) => data?.states.find((s) => s.key === key)?.count ?? 0
  return (
    <div className="space-y-4">
      <PageHeader title={t('title')}>
        <JobsNav />
      </PageHeader>
      <ErrorText error={stats.error} />
      <Panel>
        <PanelTitle>{t('dashboard.states')}</PanelTitle>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {STATE_ORDER.map((state) =>
            data ? (
              <Link
                key={state}
                to={`/jobs/list?state=${state}`}
                className="flex min-h-11 flex-col gap-1 rounded-xl border border-line bg-raised p-3 text-ink hover:border-pink"
              >
                <span className="font-display text-3xl font-bold">{number(count(state))}</span>
                <span className="text-xs text-faint">{label(state)}</span>
              </Link>
            ) : (
              <Skeleton key={state} className="h-20 rounded-xl" />
            ),
          )}
        </div>
      </Panel>
      <Panel>
        <PanelTitle>{t('dashboard.throughput', { count: hours, hours: number(hours) })}</PanelTitle>
        {data ? (
          <Suspense fallback={<Skeleton className="h-56" />}>
            <ThroughputChart buckets={data.throughput} />
          </Suspense>
        ) : (
          <Skeleton className="h-56" />
        )}
      </Panel>
      <Panel>
        <PanelTitle>{t('dashboard.queueDepth')}</PanelTitle>
        {data ? (
          data.queues.length === 0 ? (
            <p className="text-sm text-faint">{t('dashboard.noQueued')}</p>
          ) : (
            <ul className="space-y-1 font-mono text-sm">
              {data.queues.map((q) => (
                <li key={q.key} className="flex justify-between gap-4">
                  <span dir="ltr">{q.key}</span>
                  <span>{number(q.count)}</span>
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
