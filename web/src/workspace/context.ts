import { createContext, useContext } from 'react'
import type { components } from '@/api/schema'

export type WorkspaceDto = components['schemas']['WorkspaceDto']

export interface WorkspaceValue {
  /** The owner the tab acts for, or null in the user's own workspace. */
  actAs: string | null
  /** Changes on every switch; the shell keys the routed page on it so nothing carries over. */
  workspaceKey: string
  workspaces: WorkspaceDto[]
  current: WorkspaceDto | undefined
  switchTo: (owner: string | null) => void
  /** Why the tab was sent back to the own workspace, until dismissed. */
  endedNotice: string | null
  clearEndedNotice: () => void
}

export const WorkspaceContext = createContext<WorkspaceValue | null>(null)

export function useWorkspace(): WorkspaceValue {
  const value = useContext(WorkspaceContext)
  if (!value) throw new Error('WorkspaceProvider missing')
  return value
}
