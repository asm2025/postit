import { useCallback } from 'react'
import { useWorkspace } from './context'

/**
 * First elements of the query keys whose data is the same in every workspace: the signed-in
 * user's own data and the admin's. Everything else belongs to one workspace and is keyed through
 * [`wsKey`], so switching never shows A's data in B. Extend this only for data that is the
 * same in every workspace.
 */
export const SELF_KEYS: ReadonlySet<string> = new Set([
  'me',
  'workspaces',
  'preferences',
  'push',
  'notifications',
  'platforms',
  'platformIcon',
  'users',
  'audit',
  'jobs',
  'admin',
])

/** A key inside one workspace: `owner` is the acting owner's ID, or null for the user's own. */
export function wsKey(owner: string | null, ...parts: unknown[]): readonly unknown[] {
  return ['ws', owner ?? 'own', ...parts]
}

/** `wsKey` bound to the active workspace. Every workspace-scoped query uses it. */
export function useWsKey(): (...parts: unknown[]) => readonly unknown[] {
  const { actAs } = useWorkspace()
  return useCallback((...parts: unknown[]) => wsKey(actAs, ...parts), [actAs])
}
