import { useTranslation } from 'react-i18next'
import type { components } from '@/api/schema'

export type AuditKind = components['schemas']['AuditEventKindDto']

/** One entry per generated kind; a kind added to the API fails typecheck here until listed. Labels live in the `audit` catalog (`kinds.<kind>`). */
const KINDS: Record<AuditKind, true> = {
  user_provisioned: true,
  user_approved: true,
  user_disabled: true,
  user_enabled: true,
  role_changed: true,
  bootstrap_admin_granted: true,
  user_deleted: true,
  email_sent: true,
  email_dropped: true,
  email_failed: true,
  job_retried: true,
  job_cancelled: true,
  job_deleted: true,
  job_triggered: true,
  vault_rotation_started: true,
  vault_rotation_completed: true,
  account_connected: true,
  account_disconnected: true,
  account_needs_reconnect: true,
  accounts_imported: true,
  delegation_granted: true,
  delegation_changed: true,
  delegation_revoked: true,
  delegation_left: true,
  delegation_expired: true,
}

export const AUDIT_KINDS = Object.keys(KINDS) as AuditKind[]

export const isAuditKind = (v: string | null): v is AuditKind => v !== null && v in KINDS

/**
 * Clients must tolerate kinds added later (plan 02, API compatibility): an unknown kind shows
 * as the raw string. Returns the label for a kind in the screen language.
 */
export function useKindLabel(): (kind: string) => string {
  const { t } = useTranslation('audit')
  return (kind) => (isAuditKind(kind) ? t(`kinds.${kind}`) : kind)
}
