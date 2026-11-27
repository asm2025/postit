import { useWorkspace } from './context'

export type AccessLevel = 'owner' | 'view' | 'edit' | 'publish'
export type DelegatedLevel = Exclude<AccessLevel, 'owner'>

const RANK: Record<DelegatedLevel, number> = { view: 1, edit: 2, publish: 3 }

export interface Access {
  /** The tab works in the user's own workspace. */
  own: boolean
  level: AccessLevel
  /** Whether the grant reaches at least `level`; always true in the own workspace. */
  atLeast: (level: DelegatedLevel) => boolean
  /** Whether the grant reaches this account; always true in the own workspace. */
  inLimit: (accountId: string) => boolean
}

/** What the signed-in user may do in the active workspace, for gating actions in the UI. */
export function useAccess(): Access {
  const { current } = useWorkspace()
  if (!current || current.own) {
    return { own: true, level: 'owner', atLeast: () => true, inLimit: () => true }
  }
  const level: DelegatedLevel = current.level ?? 'view'
  const reach = current.all_accounts ? null : new Set(current.account_ids ?? [])
  return {
    own: false,
    level,
    atLeast: (needed) => RANK[level] >= RANK[needed],
    inLimit: (accountId) => reach === null || reach.has(accountId),
  }
}
