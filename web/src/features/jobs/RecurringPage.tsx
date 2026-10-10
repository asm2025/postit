import { useState } from 'react'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { relativeTime } from '@/lib/format'
import { JobState } from './JobState'
import { JobsNav } from './JobsNav'
import { useJobActions, useRecurringJobs } from './queries'

const when = (iso?: string | null) =>
  iso ? (
    <time dateTime={iso} title={new Date(iso).toLocaleString()}>
      {relativeTime(iso)}
    </time>
  ) : (
    '—'
  )

export function RecurringPage() {
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
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">Recurring jobs</h1>
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
          Triggered. <Link to={`/jobs/${triggered}`}>View job</Link>
        </p>
      )}
      {recurring.data?.length === 0 ? (
        <p className="text-sm text-muted-foreground">No recurring jobs are registered.</p>
      ) : (
        recurring.data && (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Schedule</TableHead>
                <TableHead>Last run</TableHead>
                <TableHead>Next run</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {recurring.data.map((r) => (
                <TableRow key={r.name}>
                  <TableCell className="font-mono text-sm">{r.name}</TableCell>
                  <TableCell className="font-mono text-sm">{r.schedule}</TableCell>
                  <TableCell>
                    <span className="inline-flex flex-wrap items-center gap-2">
                      {when(r.last_run_at)}
                      {r.last_outcome && <JobState state={r.last_outcome} />}
                    </span>
                  </TableCell>
                  <TableCell>{when(r.next_run_at)}</TableCell>
                  <TableCell>
                    <Button
                      variant="outline"
                      className="min-h-11"
                      aria-label={`Trigger ${r.name} now`}
                      onClick={() => {
                        setError(null)
                        setPending(r.name)
                      }}
                    >
                      Trigger now
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
          title="Trigger job"
          description={`Run ${pending} now? It runs once in the background.`}
          confirmLabel="Run now"
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
