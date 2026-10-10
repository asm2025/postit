import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import { App } from './App'
import { StartupError } from './StartupError'
import { completeSilentFrame, isSilentFrame } from './auth/bootstrap'
import { createUserManager } from './auth/userManager'
import { loadAuthConfig, loadRuntimeConfig } from './config/runtime'

const container = document.getElementById('root')
if (!container) throw new Error('index.html has no #root element')
const root = createRoot(container)

async function start() {
  try {
    const runtime = await loadRuntimeConfig()
    const auth = await loadAuthConfig(runtime.API_BASE_URL)
    const userManager = createUserManager(auth, window.location.origin)
    if (isSilentFrame(window)) {
      await completeSilentFrame(userManager)
      return
    }
    root.render(
      <StrictMode>
        <App config={{ runtime, auth }} userManager={userManager} />
      </StrictMode>,
    )
  } catch (e) {
    root.render(
      <StartupError
        message={e instanceof Error ? e.message : 'Unknown error'}
        onRetry={() => {
          void start()
        }}
      />,
    )
  }
}

void start()
