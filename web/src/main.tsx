import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import { App } from './App'
import { completeSilentFrame, isSilentFrame } from './auth/bootstrap'
import { createUserManager } from './auth/userManager'
import { loadAuthConfig, loadRuntimeConfig } from './config/runtime'

async function start() {
  const container = document.getElementById('root')
  if (!container) throw new Error('index.html has no #root element')
  const root = createRoot(container)
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
      <main style={{ padding: 24, fontFamily: 'sans-serif' }}>
        <h1>postit could not start</h1>
        <p>{e instanceof Error ? e.message : 'Unknown error'}</p>
      </main>,
    )
  }
}

void start()
