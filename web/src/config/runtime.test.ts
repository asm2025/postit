import { vi } from 'vitest'
import { loadAuthConfig, loadRuntimeConfig, StartupFailure } from './runtime'

const ok = (body: unknown) => vi.fn(() => Promise.resolve(new Response(JSON.stringify(body))))

it('loads /config.json relative to the page', async () => {
  const f = ok({ API_BASE_URL: 'https://api.example/', ENV: 'qa' })
  const cfg = await loadRuntimeConfig(f)
  expect(f).toHaveBeenCalledWith('/config.json', expect.anything())
  expect(cfg).toEqual({ API_BASE_URL: 'https://api.example', ENV: 'qa' })
})

it('runtime config failure carries runtime_config', async () => {
  const noUrl = loadRuntimeConfig(ok({ ENV: 'qa' }))
  await expect(noUrl).rejects.toBeInstanceOf(StartupFailure)
  await expect(noUrl).rejects.toMatchObject({ reason: 'runtime_config' })
  const down = vi.fn(() => Promise.resolve(new Response('x', { status: 503 })))
  await expect(loadRuntimeConfig(down)).rejects.toMatchObject({ reason: 'runtime_config' })
  const offline = vi.fn(() => Promise.reject(new TypeError('Failed to fetch')))
  await expect(loadRuntimeConfig(offline)).rejects.toMatchObject({ reason: 'runtime_config' })
})

it('loads auth config from the API', async () => {
  const f = ok({ issuer: 'https://idp', client_id: 'c', scopes: ['openid'] })
  const cfg = await loadAuthConfig('https://api.example', f)
  expect(f).toHaveBeenCalledWith('https://api.example/api/v1/auth/config', expect.anything())
  expect(cfg.client_id).toBe('c')
})

it('auth config failure carries auth_config', async () => {
  const f = vi.fn(() => Promise.resolve(new Response('x', { status: 500 })))
  await expect(loadAuthConfig('https://api.example', f)).rejects.toMatchObject({ reason: 'auth_config' })
  await expect(loadAuthConfig('https://api.example', ok({ issuer: 'https://idp' }))).rejects.toMatchObject({
    reason: 'auth_config',
  })
})

it('keeps the technical detail for the console, not the screen', async () => {
  const f = vi.fn(() => Promise.resolve(new Response('x', { status: 500 })))
  await expect(loadAuthConfig('https://api.example', f)).rejects.toThrow(/500/)
})
