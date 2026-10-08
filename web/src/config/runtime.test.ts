import { vi } from 'vitest'
import { loadAuthConfig, loadRuntimeConfig } from './runtime'

const ok = (body: unknown) => vi.fn(() => Promise.resolve(new Response(JSON.stringify(body))))

it('loads /config.json relative to the page', async () => {
  const f = ok({ API_BASE_URL: 'https://api.example/', ENV: 'qa' })
  const cfg = await loadRuntimeConfig(f)
  expect(f).toHaveBeenCalledWith('/config.json', expect.anything())
  expect(cfg).toEqual({ API_BASE_URL: 'https://api.example', ENV: 'qa' })
})

it('rejects a config without API_BASE_URL', async () => {
  await expect(loadRuntimeConfig(ok({ ENV: 'qa' }))).rejects.toThrow(/API_BASE_URL/)
})

it('loads auth config from the API', async () => {
  const f = ok({ issuer: 'https://idp', client_id: 'c', scopes: ['openid'] })
  const cfg = await loadAuthConfig('https://api.example', f)
  expect(f).toHaveBeenCalledWith('https://api.example/api/v1/auth/config', expect.anything())
  expect(cfg.client_id).toBe('c')
})

it('surfaces a failed load', async () => {
  const f = vi.fn(() => Promise.resolve(new Response('x', { status: 500 })))
  await expect(loadAuthConfig('https://api.example', f)).rejects.toThrow(/500/)
})
