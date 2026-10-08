import { QueryClientProvider } from '@tanstack/react-query'
import type { UserManager } from 'oidc-client-ts'
import { useMemo, useState } from 'react'
import { RouterProvider } from 'react-router'
import { ApiContext } from '@/api/ApiContext'
import { createApiClient } from '@/api/client'
import { createQueryClient } from '@/api/queryClient'
import { SessionProvider } from '@/auth/SessionProvider'
import { createTokenSource } from '@/auth/tokenSource'
import { ConfigContext, type AppConfig } from '@/config/ConfigContext'
import { createRouter } from '@/routes'
import { ThemeProvider } from '@/theme/ThemeProvider'

export function App({ config, userManager }: { config: AppConfig; userManager: UserManager }) {
  const queryClient = useMemo(() => createQueryClient(), [])
  const [expired, setExpired] = useState(false)
  const api = useMemo(() => {
    // A 401 that renewal could not fix: drop cached data and tell the Sign-in page why.
    const source = createTokenSource(userManager, () => {
      queryClient.clear()
      setExpired(true)
    })
    return createApiClient(config.runtime.API_BASE_URL, source)
  }, [config, userManager, queryClient])
  const router = useMemo(() => createRouter(), [])
  return (
    <ThemeProvider>
      <ConfigContext.Provider value={config}>
        <QueryClientProvider client={queryClient}>
          <ApiContext.Provider value={api}>
            <SessionProvider userManager={userManager} expired={expired}>
              <RouterProvider router={router} />
            </SessionProvider>
          </ApiContext.Provider>
        </QueryClientProvider>
      </ConfigContext.Provider>
    </ThemeProvider>
  )
}
