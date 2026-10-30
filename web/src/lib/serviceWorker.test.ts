import { vi } from 'vitest'
import { registerServiceWorker } from './serviceWorker'

afterEach(() => {
  Reflect.deleteProperty(navigator, 'serviceWorker')
})

it('registers /sw.js at the root when the browser supports service workers', () => {
  const register = vi.fn(() => Promise.resolve({}))
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { register } })
  registerServiceWorker()
  expect(register).toHaveBeenCalledWith('/sw.js')
})

it('swallows a failed registration', async () => {
  const register = vi.fn(() => Promise.reject(new Error('blocked')))
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { register } })
  expect(() => {
    registerServiceWorker()
  }).not.toThrow()
  await Promise.resolve()
})

it('does nothing without service worker support', () => {
  expect(() => {
    registerServiceWorker()
  }).not.toThrow()
})
