import { useTranslation } from 'react-i18next'
import { isNotificationKind } from '@/features/me/notificationKinds'

/** A kind's label in the screen language; a kind this client does not know yet shows as the raw string. */
export function useKindLabel(): (kind: string) => string {
  const { t } = useTranslation('notifications')
  return (kind) => (isNotificationKind(kind) ? t(`kind.${kind}`) : kind)
}
