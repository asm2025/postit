import type { components } from '@/api/schema'

export type AuditKind = components['schemas']['AuditEventKindDto']

/** One label per generated kind; a kind added to the API fails typecheck here until labelled. */
export const KIND_LABELS: Record<AuditKind, string> = {
  user_provisioned: 'User signed up',
  user_approved: 'User approved',
  user_disabled: 'User disabled',
  user_enabled: 'User re-enabled',
  role_changed: 'Role changed',
  bootstrap_admin_granted: 'Bootstrap admin granted',
  user_deleted: 'User deleted',
  email_sent: 'Email sent',
  email_dropped: 'Email dropped',
  email_failed: 'Email failed',
}

export const AUDIT_KINDS = Object.keys(KIND_LABELS) as AuditKind[]

export const isAuditKind = (v: string | null): v is AuditKind => v !== null && v in KIND_LABELS

/** Clients must tolerate kinds added later (plan 02, API compatibility): show the raw string. */
export function kindLabel(kind: string): string {
  return isAuditKind(kind) ? KIND_LABELS[kind] : kind
}
