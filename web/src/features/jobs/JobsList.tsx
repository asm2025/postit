import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { DataTable } from '@/components/kit/DataTable'
import { DateRangeFilter } from '@/components/kit/DateRangeFilter'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { PageHeader } from '@/components/kit/PageHeader'
import { pillClass } from '@/components/kit/pill'
import { columnHelper } from '@/components/kit/tableColumns'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { attemptsText, isJobState, STATE_ORDER, summaryText, useStateLabel } from './labels'
import { useJobs, useJobStats, type Job } from './queries'

const iso = (local: string) => {
  if (!local) return undefined
  const d = new Date(local)
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString()
}

export function JobsList() {
  const { t } = useTranslation('jobs')
  const { relative, dateTime, number } = useFormat()
  const label = useStateLabel()
  const list = useListParams()
  const stateParam = list.get('state')
  const state = isJobState(stateParam) ? stateParam : undefined
  const jobType = list.get('type')
  const from = list.get('from')
  const to = list.get('to')
  const jobs = useJobs({
    state,
    jobType: jobType || undefined,
    from: iso(from),
    to: iso(to),
    page: list.page,
    pageSize: list.pageSize,
  })
  const stats = useJobStats({ interval: 30_000 })
  const types = stats.data?.types.map((t) => t.key) ?? []
  if (jobType && !types.includes(jobType)) types.push(jobType)

  const col = columnHelper<Job>()
  const columns = col.columns([
    col.accessor('state', { header: t('list.columns.state'), cell: ({ getValue }) => <JobState state={getValue()} /> }),
    col.accessor('job_type', {
      header: t('list.columns.type'),
      meta: { ltr: true },
      cell: ({ row: { original: j } }) => (
        <Link className="font-mono hover:underline" to={`/jobs/${j.id}`}>
          {j.job_type}
        </Link>
      ),
    }),
    col.accessor('summary', {
      header: t('list.columns.summary'),
      meta: { className: 'max-w-xs truncate text-muted-foreground' },
      cell: ({ getValue }) => <bdi dir="auto">{summaryText(getValue())}</bdi>,
    }),
    col.display({
      id: 'attempts',
      header: t('list.columns.attempts'),
      meta: { className: 'tabular-nums' },
      cell: ({ row: { original: j } }) => attemptsText(j.attempts, j.max_attempts, number),
    }),
    col.display({
      id: 'time',
      header: t('list.columns.time'),
      cell: ({ row: { original: j } }) => {
        const waiting = j.state === 'queued' || j.state === 'scheduled'
        const when = waiting ? j.run_at : (j.finished_at ?? j.run_at)
        return (
          <time dateTime={when} title={dateTime(when)}>
            {relative(when)}
          </time>
        )
      },
    }),
  ])

  return (
    <div className="space-y-4">
      <PageHeader title={t('title')}>
        <JobsNav />
      </PageHeader>
      <div role="tablist" aria-label={t('list.stateLabel')} className="flex flex-wrap gap-1.5">
        {[undefined, ...STATE_ORDER].map((s) => (
          <button
            key={s ?? 'all'}
            type="button"
            role="tab"
            aria-selected={s === state}
            className={pillClass(s === state)}
            onClick={() => {
              list.set({ state: s })
            }}
          >
            {s ? label(s) : t('list.all')}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-end gap-2">
        <FormField label={t('list.jobType')}>
          <OptionSelect
            value={jobType}
            anyLabel={t('list.anyType')}
            options={types.map((type) => ({ value: type, label: type, dir: 'ltr' as const }))}
            onChange={(v) => {
              list.set({ type: v })
            }}
          />
        </FormField>
        <DateRangeFilter
          from={from}
          to={to}
          onFromChange={(v) => {
            list.set({ from: v })
          }}
          onToChange={(v) => {
            list.set({ to: v })
          }}
        />
      </div>
      <ErrorText error={jobs.error} />
      <DataTable
        label={t('title')}
        columns={columns}
        data={jobs.data?.data}
        getRowId={(j) => j.id}
        empty={<EmptyState>{t('list.none')}</EmptyState>}
        pagination={jobs.data && list.paging(jobs.data.total)}
      />
    </div>
  )
}
