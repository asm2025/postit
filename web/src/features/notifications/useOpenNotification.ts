import { useCallback } from 'react'
import { useNavigate } from 'react-router'
import { useMe } from '@/features/me/useMe'
import { useWorkspace } from '@/workspace/context'
import { linkFor } from './links'
import { useMarkRead, type NotificationDto } from './queries'

/**
 * Opens a notification: marks it read, switches to its workspace when the link belongs to one
 * the tab is not in, then goes to the page, or to the list where there is no page yet.
 */
export function useOpenNotification(): (n: NotificationDto) => void {
  const me = useMe()
  const { actAs, switchTo } = useWorkspace()
  const navigate = useNavigate()
  const { mutate: markRead } = useMarkRead()
  const myId = me.data?.id ?? ''
  return useCallback(
    (n) => {
      if (!n.read_at) markRead({ ids: [n.id] })
      const link = linkFor(n, myId)
      if (!link) {
        void navigate('/notifications')
        return
      }
      if (link.scope === 'owner' && n.owner_id && n.owner_id !== (actAs ?? myId)) {
        switchTo(n.owner_id === myId ? null : n.owner_id)
      }
      void navigate(link.path)
    },
    [actAs, markRead, myId, navigate, switchTo],
  )
}
