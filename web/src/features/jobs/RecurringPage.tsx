import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DataTable } from '@/components/kit/DataTable'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { columnHelper } from '@/components/kit/tableColumns'
import { Button } from '@/components/ui/button'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { useJobActions, useRecurringJobs, type RecurringJob } from './queries'

function When({ iso }: { iso?: string | null }) {
  const { relative, dateTime } = useFormat()
  return iso ? (
    <time dateTime={iso} title={dateTime(iso)}>
      {relative(iso)}
    </time>
  ) : (
    '—'
  )
}

export function RecurringPage() {
  const { t } = useTranslation('jobs')
  const recurring = useRecurringJobs()
  const { trigger } = useJobActions()
  const [pending, setPending] = useState<string | null>(null)
  const [error, setError] = useState<unknown>(null)
  const [triggered, setTriggered] = useState<string | null>(null)

  const run = (name: string) => {
    setError(null)
    setTriggered(null)
    trigger.mutate(name, {
      onSuccess: (id) => {
        setPending(null)
        setTriggered(id)
      },
      onError: (e) => {
        setPending(null)
        setError(e)
      },
    })
  }

  const col = columnHelper<RecurringJob>()
  const columns = col.columns([
    col.accessor('name', {
      header: t('recurring.columns.name'),
      enableSorting: true,
      meta: { className: 'font-mono text-sm', ltr: true },
    }),
    col.accessor('schedule', {
      header: t('recurring.columns.schedule'),
      meta: { className: 'font-mono text-sm', ltr: true },
    }),
    col.display({
      id: 'lastRun',
      header: t('recurring.columns.lastRun'),
      cell: ({ row: { original: r } }) => (
        <span className="inline-flex flex-wrap items-center gap-2">
          <When iso={r.last_run_at} />
          {r.last_outcome && <JobState state={r.last_outcome} />}
        </span>
      ),
    }),
    col.accessor('next_run_at', {
      header: t('recurring.columns.nextRun'),
      enableSorting: true,
      cell: ({ getValue }) => <When iso={getValue()} />,
    }),
    col.display({
      id: 'actions',
      header: () => <span className="sr-only">{t('recurring.columns.actions')}</span>,
      cell: ({ row: { original: r } }) => (
        <Button
          variant="outline"
          aria-label={t('recurring.triggerAria', { name: r.name })}
          onClick={() => {
            setError(null)
            setPending(r.name)
          }}
        >
          {t('recurring.triggerNow')}
        </Button>
      ),
    }),
  ])

  return (
    <div className="space-y-4">
      <PageHeader title={t('recurring.title')}>
        <JobsNav />
      </PageHeader>
      <ErrorText error={recurring.error} />
      <ErrorText error={error} />
      {triggered && (
        <p role="status" className="text-sm">
          {t('recurring.triggered')} <Link to={`/jobs/${triggered}`}>{t('recurring.viewJob')}</Link>
        </p>
      )}
      <DataTable
        label={t('recurring.title')}
        columns={columns}
        data={recurring.data}
        getRowId={(r) => r.name}
        empty={<EmptyState>{t('recurring.none')}</EmptyState>}
      />
      {pending && (
        <ConfirmDialog
          open
          title={t('recurring.triggerTitle')}
          description={t('recurring.triggerBody', { name: isolate(pending) })}
          confirmLabel={t('recurring.runNow')}
          destructive={false}
          busy={trigger.isPending}
          onConfirm={() => {
            run(pending)
          }}
          onCancel={() => {
            setPending(null)
          }}
        />
      )}
    </div>
  )
}
