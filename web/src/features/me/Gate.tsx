import { useQueryClient } from '@tanstack/react-query'
import { Navigate, Outlet, useLocation } from 'react-router'
import { ApiError, messageFor } from '@/api/errors'
import { useSession } from '@/auth/session'
import { DisabledScreen, ErrorScreen, Forbidden, Loading, PendingScreen } from './StatusScreens'
import { useMe } from './useMe'

export function RequireSession() {
  const { status } = useSession()
  const location = useLocation()
  if (status === 'loading') return <Loading />
  if (status === 'signedOut') {
    return <Navigate to="/login" replace state={{ from: location.pathname + location.search }} />
  }
  return <Outlet />
}

/** Renders the app only for active users; shows the matching screen otherwise. */
export function Gate() {
  const me = useMe()
  const qc = useQueryClient()
  const retry = () => void qc.invalidateQueries({ queryKey: ['me'] })
  if (me.isPending) return <Loading />
  // Checked before `me.data`: TanStack keeps the old data next to a failed refetch.
  if (me.error) {
    const code = me.error instanceof ApiError ? me.error.code : ''
    if (code === 'account_disabled') return <DisabledScreen />
    if (code === 'account_pending') return <PendingScreen onRefresh={retry} />
    return <ErrorScreen message={messageFor(me.error)} onRetry={retry} />
  }
  switch (me.data.status) {
    case 'pending':
      return <PendingScreen onRefresh={retry} />
    case 'disabled':
    case 'deleting':
      return <DisabledScreen />
    default:
      return <Outlet />
  }
}

export function RequireAdmin() {
  const me = useMe()
  if (me.isPending) return <Loading />
  if (me.data?.role !== 'admin') return <Forbidden />
  return <Outlet />
}
