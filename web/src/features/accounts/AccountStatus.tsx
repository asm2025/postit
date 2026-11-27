import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import type { Account } from './queries'

const TONE: Record<Account['status'], string> = {
  active: 'bg-muted text-muted-foreground',
  needs_reconnect: 'bg-warn-bg text-warn-text',
  revoked: 'bg-warn-bg text-warn-text',
  disconnected: 'bg-muted text-faint',
}

/** An account's status as a colored badge. */
export function AccountStatus({ status }: { status: Account['status'] }) {
  const { t } = useTranslation('accounts')
  return <Badge className={TONE[status]}>{t(`status.${status}`)}</Badge>
}
