import { useEffect } from 'react'
import { useNavigate } from 'react-router'
import { RETURN_TO_KEY, useSession } from '@/auth/session'
import { Loading } from './StatusScreens'

/**
 * The provider has already processed the response; this page only routes, and replaces the
 * URL so the code, state or IdP error never stays in it. It never retries on its own.
 */
export function CallbackPage() {
  const { status } = useSession()
  const navigate = useNavigate()
  useEffect(() => {
    if (status === 'signedIn') {
      let to = '/'
      try {
        to = sessionStorage.getItem(RETURN_TO_KEY) ?? '/'
        sessionStorage.removeItem(RETURN_TO_KEY)
      } catch {
        /* ignore */
      }
      // Only same-origin paths; `//host` would be protocol-relative.
      void navigate(to.startsWith('/') && !to.startsWith('//') ? to : '/', { replace: true })
    } else if (status === 'signedOut') {
      // The Sign-in page shows the session's notice (IdP error or state mismatch).
      void navigate('/login', { replace: true })
    }
  }, [status, navigate])
  return <Loading />
}
