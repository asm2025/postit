import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ErrorText } from '@/components/kit/ErrorText'
import { Button } from '@/components/ui/button'
import { type Job, useJobActions } from './queries'

// The server's English sentences, each mapped to a catalog line; anything else gets the generic one.
const REASON_KEYS: Record<
  string,
  'actions.reason.unknownType' | 'actions.reason.retried' | 'actions.reason.deleteUser'
> = {
  'job type is not registered in this release': 'actions.reason.unknownType',
  'job was already retried': 'actions.reason.retried',
  'erasing a user must finish; it can only be retried': 'actions.reason.deleteUser',
}

type Action = 'retry' | 'cancel' | 'delete'

const DESTRUCTIVE: Record<Action, boolean> = { retry: false, cancel: true, delete: true }

/** Buttons come only from `job.actions`; the server decides what is allowed. */
export function JobActions({ job }: { job: Job }) {
  const { t } = useTranslation('jobs')
  const { retry, cancel, remove } = useJobActions()
  const navigate = useNavigate()
  const [pending, setPending] = useState<Action | null>(null)
  const [error, setError] = useState<unknown>(null)
  const shown = (['retry', 'cancel', 'delete'] as const).filter((a) => job.actions[a])
  const busy = retry.isPending || cancel.isPending || remove.isPending

  const run = (action: Action) => {
    setError(null)
    const fail = (e: unknown) => {
      setPending(null)
      setError(e)
    }
    if (action === 'retry')
      retry.mutate(job.id, {
        onSuccess: (newId) => {
          setPending(null)
          void navigate(`/jobs/${newId}`)
        },
        onError: fail,
      })
    else if (action === 'cancel')
      cancel.mutate(job.id, {
        onSuccess: () => {
          setPending(null)
        },
        onError: fail,
      })
    else
      remove.mutate(job.id, {
        onSuccess: () => {
          setPending(null)
          void navigate('/jobs/list')
        },
        onError: fail,
      })
  }

  return (
    <div className="space-y-2">
      {shown.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {shown.map((a) => (
            <Button
              key={a}
              variant={a === 'retry' ? 'default' : 'outline'}
              onClick={() => {
                setError(null)
                setPending(a)
              }}
            >
              {t(`actions.${a}`)}
            </Button>
          ))}
        </div>
      ) : (
        job.actions_disabled_reason && (
          <p className="text-sm text-muted-foreground">
            {t(REASON_KEYS[job.actions_disabled_reason] ?? 'actions.reason.generic')}
          </p>
        )
      )}
      <ErrorText error={error} />
      {pending && (
        <ConfirmDialog
          open
          title={t(`actions.${pending}Title`)}
          description={t(`actions.${pending}Body`)}
          confirmLabel={t(`actions.${pending}`)}
          destructive={DESTRUCTIVE[pending]}
          busy={busy}
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
