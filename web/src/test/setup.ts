import '@testing-library/jest-dom/vitest'
import { afterAll, afterEach, beforeAll, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import { initI18n, setLocale } from '@/i18n'
import { server } from './server'

// recharts' ResponsiveContainer observes its parent; jsdom has no ResizeObserver.
class ResizeObserverStub {
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()
}
vi.stubGlobal('ResizeObserver', ResizeObserverStub)

beforeAll(async () => {
  await initI18n()
  await setLocale('en-US')
  server.listen({ onUnhandledRequest: 'error' })
})
afterEach(() => {
  cleanup()
  server.resetHandlers()
})
afterAll(() => {
  server.close()
})
