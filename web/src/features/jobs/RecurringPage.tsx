import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { useJobActions, useRecurringJobs } from './queries'

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

  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">{t('recurring.title')}</h1>
      <JobsNav />
      {recurring.isError && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(recurring.error)}
        </p>
      )}
      {error !== null && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(error)}
        </p>
      )}
      {triggered && (
        <p role="status" className="text-sm">
          {t('recurring.triggered')} <Link to={`/jobs/${triggered}`}>{t('recurring.viewJob')}</Link>
        </p>
      )}
      {recurring.data?.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('recurring.none')}</p>
      ) : (
        recurring.data && (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('recurring.columns.name')}</TableHead>
                <TableHead>{t('recurring.columns.schedule')}</TableHead>
                <TableHead>{t('recurring.columns.lastRun')}</TableHead>
                <TableHead>{t('recurring.columns.nextRun')}</TableHead>
                <TableHead>
                  <span className="sr-only">{t('recurring.columns.actions')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {recurring.data.map((r) => (
                <TableRow key={r.name}>
                  <TableCell className="font-mono text-sm" dir="ltr">
                    {r.name}
                  </TableCell>
                  <TableCell className="font-mono text-sm" dir="ltr">
                    {r.schedule}
                  </TableCell>
                  <TableCell>
                    <span className="inline-flex flex-wrap items-center gap-2">
                      <When iso={r.last_run_at} />
                      {r.last_outcome && <JobState state={r.last_outcome} />}
                    </span>
                  </TableCell>
                  <TableCell>
                    <When iso={r.next_run_at} />
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="outline"
                      className="min-h-11"
                      aria-label={t('recurring.triggerAria', { name: r.name })}
                      onClick={() => {
                        setError(null)
                        setPending(r.name)
                      }}
                    >
                      {t('recurring.triggerNow')}
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )
      )}
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
