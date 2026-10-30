import { StrictMode } from 'react'
import i18next from 'i18next'
import { createRoot } from 'react-dom/client'
import './index.css'
import { App } from './App'
import { StartupError } from './StartupError'
import { createUserManager } from './auth/userManager'
import { initI18n } from './i18n'
import { registerServiceWorker } from './lib/serviceWorker'
import { loadAuthConfig, loadRuntimeConfig } from './config/runtime'

const container = document.getElementById('root')
if (!container) throw new Error('index.html has no #root element')
const root = createRoot(container)

// If i18n itself failed to initialize there is no catalog to read, so keep the English literal.
const unknownError = () => (i18next.isInitialized ? i18next.t('startup.unknown') : 'Unknown error')

async function start() {
  try {
    await initI18n()
    const runtime = await loadRuntimeConfig()
    const auth = await loadAuthConfig(runtime.API_BASE_URL)
    const userManager = createUserManager(auth, window.location.origin)
    root.render(
      <StrictMode>
        <App config={{ runtime, auth }} userManager={userManager} />
      </StrictMode>,
    )
    // Web Push needs the service worker; registered once the app has started.
    registerServiceWorker()
  } catch (e) {
    root.render(
      <StartupError
        message={e instanceof Error ? e.message : unknownError()}
        onRetry={() => {
          void start()
        }}
      />,
    )
  }
}

void start()
