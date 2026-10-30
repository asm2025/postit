import { useCallback, useRef } from 'react'
import { useApi } from '@/api/ApiContext'
import { useSession } from '@/auth/session'
import { disablePush } from './useBrowserPush'

/**
 * Signs out after a best-effort removal of this browser's push registration, so the next person on a
 * shared browser does not receive this account's notifications. A failure or a slow server never holds
 * sign-out back: it waits at most `timeoutMs`.
 */
export function useSignOut(timeoutMs = 3000): () => void {
  const api = useApi()
  const { signOut } = useSession()
  const running = useRef(false)
  return useCallback(() => {
    if (running.current) return
    running.current = true
    void (async () => {
      let timer: ReturnType<typeof setTimeout> | undefined
      const limit = new Promise<void>((resolve) => {
        timer = setTimeout(resolve, timeoutMs)
      })
      try {
        await Promise.race([disablePush(api).catch(() => undefined), limit])
      } finally {
        clearTimeout(timer)
        running.current = false
        signOut()
      }
    })()
  }, [api, signOut, timeoutMs])
}
