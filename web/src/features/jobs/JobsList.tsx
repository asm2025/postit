import { useTranslation } from 'react-i18next'
import { Link, useSearchParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { Pager } from '@/components/Pager'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useFormat } from '@/i18n/useFormat'
import { cn } from 'cn'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { attemptsText, isJobState, STATE_ORDER, summaryText, useStateLabel } from './labels'
import { useJobs, useJobStats } from './queries'

const iso = (local: string) => {
  if (!local) return undefined
  const d = new Date(local)
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString()
}

const tabClass = (active: boolean) =>
  cn(
    'inline-flex min-h-11 items-center rounded-lg border px-3.5 text-sm font-medium',
    active ? 'border-pink bg-pink-soft text-ink' : 'border-line bg-surface text-ink hover:bg-raised',
  )

export function JobsList() {
  const { t } = useTranslation('jobs')
  const { relative, dateTime, number } = useFormat()
  const label = useStateLabel()
  const [params, setParams] = useSearchParams()
  const stateParam = params.get('state')
  const state = isJobState(stateParam) ? stateParam : undefined
  const jobType = params.get('type') ?? ''
  const from = params.get('from') ?? ''
  const to = params.get('to') ?? ''
  const page = Math.max(1, Number(params.get('page') ?? 1) || 1)
  const jobs = useJobs({ state, jobType: jobType || undefined, from: iso(from), to: iso(to), page })
  const stats = useJobStats({ interval: 30_000 })
  const types = stats.data?.types.map((t) => t.key) ?? []
  if (jobType && !types.includes(jobType)) types.push(jobType)

  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value)
    else next.delete(key)
    if (key !== 'page') next.delete('page')
    setParams(next, { replace: true })
  }

  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">{t('title')}</h1>
      <JobsNav />
      <div role="tablist" aria-label={t('list.stateLabel')} className="flex flex-wrap gap-1.5">
        {[undefined, ...STATE_ORDER].map((s) => (
          <button
            key={s ?? 'all'}
            type="button"
            role="tab"
            aria-selected={s === state}
            className={tabClass(s === state)}
            onClick={() => {
              set('state', s ?? '')
            }}
          >
            {s ? label(s) : t('list.all')}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-end gap-2 text-sm">
        <label className="flex flex-col gap-1">
          {t('list.jobType')}
          <select
            className="min-h-11 rounded-lg border border-line bg-surface px-2.5"
            value={jobType}
            onChange={(e) => {
              set('type', e.target.value)
            }}
          >
            <option value="">{t('list.anyType')}</option>
            {types.map((type) => (
              <option key={type} value={type} dir="ltr">
                {type}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          {t('list.from')}
          <Input
            type="datetime-local"
            value={from}
            onChange={(e) => {
              set('from', e.target.value)
            }}
          />
        </label>
        <label className="flex flex-col gap-1">
          {t('list.to')}
          <Input
            type="datetime-local"
            value={to}
            onChange={(e) => {
              set('to', e.target.value)
            }}
          />
        </label>
      </div>
      {jobs.error && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(jobs.error)}
        </p>
      )}
      {jobs.data && jobs.data.data.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('list.columns.state')}</TableHead>
              <TableHead>{t('list.columns.type')}</TableHead>
              <TableHead>{t('list.columns.summary')}</TableHead>
              <TableHead>{t('list.columns.attempts')}</TableHead>
              <TableHead>{t('list.columns.time')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {jobs.data.data.map((j) => {
              const waiting = j.state === 'queued' || j.state === 'scheduled'
              const when = waiting ? j.run_at : (j.finished_at ?? j.run_at)
              return (
                <TableRow key={j.id}>
                  <TableCell>
                    <JobState state={j.state} />
                  </TableCell>
                  <TableCell>
                    <Link className="font-mono hover:underline" dir="ltr" to={`/jobs/${j.id}`}>
                      {j.job_type}
                    </Link>
                  </TableCell>
                  <TableCell className="max-w-xs truncate text-muted-foreground" dir="auto">
                    {summaryText(j.summary)}
                  </TableCell>
                  <TableCell className="font-mono">{attemptsText(j.attempts, j.max_attempts, number)}</TableCell>
                  <TableCell>
                    <time dateTime={when} title={dateTime(when)}>
                      {relative(when)}
                    </time>
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      )}
      {jobs.data?.data.length === 0 && <p className="text-sm text-muted-foreground">{t('list.none')}</p>}
      {jobs.data && (
        <Pager
          page={page}
          pageSize={jobs.data.page_size}
          total={jobs.data.total}
          onPage={(p) => {
            set('page', String(p))
          }}
        />
      )}
    </div>
  )
}
