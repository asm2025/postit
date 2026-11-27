import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { messageFor } from '@/api/errors'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import type { components } from '@/api/schema'
import { clearPending } from './pendingStore'
import { useConfirmConnection, useDiscardConnection } from './queries'

type Discovered = components['schemas']['DiscoveredAccountDto']

const TICK_MS = 15_000

/** Whole minutes left until `expiresAt`, never below zero; re-evaluated on a timer. */
function useMinutesLeft(expiresAt: string): number {
  const left = () => Math.max(0, Math.ceil((Date.parse(expiresAt) - Date.now()) / 60_000))
  const [minutes, setMinutes] = useState(left)
  useEffect(() => {
    const timer = setInterval(() => {
      setMinutes(Math.max(0, Math.ceil((Date.parse(expiresAt) - Date.now()) / 60_000)))
    }, TICK_MS)
    return () => {
      clearInterval(timer)
    }
  }, [expiresAt])
  return minutes
}

/**
 * The accounts a started connection reached, each with a box, all ticked. Confirming connects the
 * ticked ones; canceling throws the connection away. The one-time `connect_id` never leaves
 * memory and `sessionStorage`.
 */
export function DiscoveryChecklist({
  connectId,
  pendingId,
  discovered,
  expiresAt,
  onDone,
}: {
  connectId: string
  pendingId: string | null
  discovered: Discovered[]
  expiresAt: string
  /** Called once the user is through: connected, canceled, or too late. */
  onDone: (result: 'connected' | 'canceled' | 'expired') => void
}) {
  const { t } = useTranslation(['accounts', 'common'])
  const navigate = useNavigate()
  const minutes = useMinutesLeft(expiresAt)
  const [chosen, setChosen] = useState(() => discovered.map((a) => a.remote_id))
  const confirm = useConfirmConnection(connectId)
  const discard = useDiscardConnection()

  if (minutes <= 0) {
    return (
      <div className="flex flex-col items-start gap-3">
        <EmptyState>{t('accounts:checklist.expired')}</EmptyState>
        <Button
          variant="outline"
          onClick={() => {
            clearPending()
            onDone('expired')
          }}
        >
          {t('accounts:checklist.startAgain')}
        </Button>
      </div>
    )
  }

  const toggle = (id: string, on: boolean) => {
    setChosen(on ? [...chosen, id] : chosen.filter((c) => c !== id))
  }
  const finish = (result: 'connected' | 'canceled') => {
    clearPending()
    onDone(result)
  }

  return (
    <section aria-labelledby="checklist-title" className="flex max-w-lg flex-col gap-4">
      <div>
        <h2 id="checklist-title" className="font-display text-xl font-semibold">
          {t('accounts:checklist.title')}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t('accounts:checklist.intro')} {t('accounts:checklist.minutesLeft', { count: minutes })}
        </p>
      </div>
      <ul className="flex flex-col gap-2">
        {discovered.map((a) => {
          const id = `discovered-${a.remote_id}`
          return (
            <li key={a.remote_id} className="flex items-center gap-3">
              <Checkbox
                id={id}
                checked={chosen.includes(a.remote_id)}
                onCheckedChange={(checked) => {
                  toggle(a.remote_id, checked === true)
                }}
              />
              {a.avatar_url ? (
                <img src={a.avatar_url} alt="" className="size-8 rounded-full" />
              ) : (
                <span
                  aria-hidden="true"
                  className="flex size-8 items-center justify-center rounded-full bg-raised text-xs font-bold"
                >
                  {a.display_name.slice(0, 1).toUpperCase()}
                </span>
              )}
              <label htmlFor={id} className="flex flex-col text-sm">
                <span dir="auto" className="font-medium">
                  {a.display_name}
                </span>
                {a.handle && (
                  <span dir="ltr" className="text-xs text-muted-foreground">
                    {a.handle}
                  </span>
                )}
              </label>
            </li>
          )
        })}
      </ul>
      <ErrorText error={confirm.error ? messageFor(confirm.error) : discard.error ? messageFor(discard.error) : null} />
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={chosen.length === 0 || confirm.isPending}
          onClick={() => {
            confirm.mutate(chosen, {
              onSuccess: (done) => {
                const message = [
                  t('accounts:checklist.connected', { count: done.connected.length }),
                  done.skipped_elsewhere > 0
                    ? t('accounts:checklist.skippedElsewhere', { count: done.skipped_elsewhere })
                    : '',
                ]
                  .filter(Boolean)
                  .join(' ')
                finish('connected')
                void navigate('/accounts', { state: { notice: message } })
              },
            })
          }}
        >
          {t('common:actions.confirm')}
        </Button>
        <Button
          variant="outline"
          disabled={discard.isPending}
          onClick={() => {
            // Without a row to delete the connection is only forgotten here.
            if (pendingId === null) {
              finish('canceled')
              return
            }
            discard.mutate(pendingId, {
              onSuccess: () => {
                finish('canceled')
              },
            })
          }}
        >
          {t('common:actions.cancel')}
        </Button>
      </div>
    </section>
  )
}
