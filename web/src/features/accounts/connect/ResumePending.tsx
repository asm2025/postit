import { useEffect, useMemo, useState } from 'react'
import { useMe } from '@/features/me/useMe'
import { DiscoveryChecklist } from './DiscoveryChecklist'
import { clearPending, loadPending } from './pendingStore'
import { useConnectInfo } from './queries'

/**
 * A reload inside a connection's ten minutes shows its checklist again. An entry that expired,
 * or belongs to another user in this tab, is dropped and shows nothing.
 */
export function ResumePending() {
  const me = useMe()
  const userId = me.data?.id
  const entry = useMemo(() => (userId ? loadPending(userId) : null), [userId])
  const info = useConnectInfo(entry?.connectId ?? null)
  const [done, setDone] = useState(false)
  const gone = info.isError
  useEffect(() => {
    if (gone) clearPending()
  }, [gone])
  if (!entry || done || gone || !info.data) return null
  return (
    <DiscoveryChecklist
      connectId={entry.connectId}
      pendingId={info.data.pending_id}
      discovered={info.data.discovered}
      expiresAt={info.data.expires_at}
      onDone={() => {
        setDone(true)
      }}
    />
  )
}
