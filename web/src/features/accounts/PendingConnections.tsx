import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ErrorText } from '@/components/kit/ErrorText'
import { Button } from '@/components/ui/button'
import { useFormat } from '@/i18n/useFormat'
import { usePlatforms } from '@/features/platforms/queries'
import { useCancelPending, usePendingConnections } from './queries'

/**
 * Connections the owner started and left. A lost `connect_id` cannot be recovered, so the only
 * ways on are to cancel or to start again; an unfinished connection is never confirmed from here.
 */
export function PendingConnections() {
  const { t } = useTranslation(['accounts', 'common'])
  const { relative } = useFormat()
  const pending = usePendingConnections(true)
  const platforms = usePlatforms()
  const cancel = useCancelPending()
  const rows = pending.data ?? []
  if (rows.length === 0) return null
  const name = (id: string) => platforms.data?.find((p) => p.platform === id)?.display_name ?? id
  return (
    <section aria-labelledby="pending-title" className="flex flex-col gap-2">
      <h2 id="pending-title" className="text-base font-semibold">
        {t('accounts:pending.title')}
      </h2>
      <ul className="flex flex-col gap-2">
        {rows.map((p) => (
          <li
            key={p.pending_id}
            className="flex flex-wrap items-center gap-3 rounded-xl border border-line bg-raised px-4 py-2 text-sm"
          >
            <span className="flex-1">
              {t('accounts:pending.row', { platform: name(p.platform), when: relative(p.expires_at) })}
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={cancel.isPending}
              onClick={() => {
                cancel.mutate(p.pending_id)
              }}
            >
              {t('common:actions.cancel')}
            </Button>
            <Button asChild size="sm">
              <Link to={`/accounts/connect/${p.platform}`}>{t('accounts:pending.startAgain')}</Link>
            </Button>
          </li>
        ))}
      </ul>
      <ErrorText error={cancel.error ? messageFor(cancel.error) : null} />
    </section>
  )
}
