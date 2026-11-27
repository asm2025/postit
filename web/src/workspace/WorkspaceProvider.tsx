import { useQuery, useQueryClient, type Query } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation, useNavigate } from 'react-router'
import type { ReactNode } from 'react'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import { ErrorScreen, Loading } from '@/features/me/StatusScreens'
import { messageFor } from '@/api/errors'
import { WorkspaceContext, type WorkspaceValue } from './context'
import { SELF_KEYS } from './keys'
import { keepsOnSwitch } from './paths'
import { getActAs, setActAs, subscribeActAs, subscribeDelegationInvalid } from './store'

const isWorkspaceQuery = (query: Query) => !SELF_KEYS.has(String(query.queryKey[0]))

/**
 * A layout route between the gate and the shell. It owns which workspace the tab acts in and
 * everything a switch must undo: workspace data is dropped, the page remounts, and a grant that
 * ends mid-session sends the tab back to the user's own workspace.
 */
export function WorkspaceProvider() {
  return (
    <WorkspaceScope>
      <Outlet />
    </WorkspaceScope>
  )
}

/** The provider's logic around any children, for a router that does not nest routes. */
export function WorkspaceScope({ children }: { children: ReactNode }) {
  const api = useApi()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useTranslation()
  const stored = useSyncExternalStore(subscribeActAs, getActAs, () => null)
  const [generation, setGeneration] = useState(0)
  const [endedNotice, setEndedNotice] = useState<string | null>(null)
  const list = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => unwrap(api.GET('/api/v1/me/workspaces')),
  })
  const workspaces = list.data ?? []
  const known = stored === null || workspaces.some((w) => !w.own && w.owner.id === stored)

  const path = useRef(location.pathname)
  useEffect(() => {
    path.current = location.pathname
  }, [location.pathname])

  // Drops whatever is not the user's own or the admin's, including a hook that forgot its
  // workspace key, and cancels the requests still in flight for it.
  const dropWorkspaceData = useCallback(() => {
    void qc.cancelQueries({ predicate: isWorkspaceQuery })
    qc.removeQueries({ predicate: isWorkspaceQuery })
  }, [qc])

  const switchTo = useCallback(
    (owner: string | null) => {
      dropWorkspaceData()
      setActAs(owner)
      setGeneration((g) => g + 1)
      if (!keepsOnSwitch(path.current)) void navigate('/')
    },
    [dropWorkspaceData, navigate],
  )

  // A stored owner the user no longer holds a grant to: back to the own workspace. The key the
  // shell remounts on carries the owner, so the store change alone is enough.
  useEffect(() => {
    if (list.isSuccess && !known) {
      dropWorkspaceData()
      setActAs(null)
    }
  }, [list.isSuccess, known, dropWorkspaceData])

  // The server says the grant ended: once, back to the own workspace, with the reason.
  useEffect(
    () =>
      subscribeDelegationInvalid(() => {
        switchTo(null)
        setEndedNotice(t('workspace.ended'))
        void qc.invalidateQueries({ queryKey: ['workspaces'] })
      }),
    [switchTo, t, qc],
  )

  const value = useMemo<WorkspaceValue>(
    () => ({
      actAs: stored,
      workspaceKey: `${stored ?? 'own'}:${String(generation)}`,
      workspaces,
      current: workspaces.find((w) => (stored === null ? w.own : w.owner.id === stored)),
      switchTo,
      endedNotice,
      clearEndedNotice: () => {
        setEndedNotice(null)
      },
    }),
    // `workspaces` is a fresh array per render when the query has no data yet.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [stored, generation, list.data, switchTo, endedNotice],
  )

  if (list.isError && !list.data) {
    return (
      <ErrorScreen
        message={messageFor(list.error)}
        onRetry={() => {
          void list.refetch()
        }}
      />
    )
  }
  if (!list.isSuccess || !known) return <Loading />
  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>
}
