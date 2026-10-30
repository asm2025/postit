import { useTranslation } from 'react-i18next'
import type { components } from '@/api/schema'

export type NotificationKind = components['schemas']['NotificationKind']

/** One entry per generated kind; a kind added to the API fails typecheck here until listed. Names live in the `me` catalog (`notifications.kinds.names.<kind>`). */
const KINDS: Record<NotificationKind, true> = {
  delivery_dead_lettered: true,
  delivery_failed: true,
  delivery_outcome_unknown: true,
  delivery_parked: true,
  delivery_stale: true,
  job_partial: true,
  account_needs_reconnect: true,
  account_revoked_by_platform: true,
  reconnect_by_passed: true,
  recurrence_skipped: true,
  delivery_published: true,
  reconnect_reminder: true,
  delegation_changed: true,
  new_comments: true,
  user_pending: true,
}

export const NOTIFICATION_KINDS = Object.keys(KINDS) as NotificationKind[]

export const isNotificationKind = (v: string): v is NotificationKind => Object.hasOwn(KINDS, v)

/** A kind's name in the screen language; a kind the client does not know yet shows as the raw string. */
export function useKindName(): (kind: string) => string {
  const { t } = useTranslation('me')
  return (kind) => (isNotificationKind(kind) ? t(`notifications.kinds.names.${kind}`) : kind)
}
