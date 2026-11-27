import type { NotificationDto } from './queries'

/**
 * Where a notification leads. `owner` links belong to the workspace the notice is about, so
 * opening one switches there first; `self` links are about the user themselves and never switch.
 */
export interface NotificationLink {
  path: string
  scope: 'owner' | 'self'
}

/** The account a notice names, for the kinds that carry one. */
function accountOf(n: NotificationDto): string | null {
  const payload = n.payload as { account?: { account_id?: string } }
  return payload.account?.account_id ?? null
}

/** The link of a notice, or null where there is nowhere to go yet (delivery, job, recurrence, and comment kinds). */
export function linkFor(n: NotificationDto, me: string): NotificationLink | null {
  switch (n.kind) {
    case 'account_needs_reconnect':
    case 'account_revoked_by_platform':
    case 'reconnect_by_passed':
    case 'reconnect_reminder': {
      const account = accountOf(n)
      if (!account) return null
      return { path: `/accounts/${account}`, scope: n.owner_id ? 'owner' : 'self' }
    }
    case 'delegation_changed':
      // The owner of the notice is the workspace the grant opens: mine when a delegate changed or left it.
      return { path: `/delegations?tab=${n.owner_id === me ? 'granted' : 'received'}`, scope: 'self' }
    case 'user_pending':
      return { path: '/users', scope: 'self' }
    case 'delivery_dead_lettered':
    case 'delivery_failed':
    case 'delivery_outcome_unknown':
    case 'delivery_parked':
    case 'delivery_stale':
    case 'delivery_published':
    case 'job_partial':
    case 'recurrence_skipped':
    case 'new_comments':
      return null
  }
}

/** A short line naming what a notice is about: the account, or the other party of a grant. */
export function subjectOf(n: NotificationDto): string | null {
  const payload = n.payload as { account?: { account_name?: string }; party_name?: string }
  return payload.account?.account_name ?? payload.party_name ?? null
}
