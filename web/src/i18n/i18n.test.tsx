import { http, HttpResponse } from 'msw'
import { renderHook } from '@testing-library/react'
import { ApiError, messageFor } from '@/api/errors'
import { createAuthFetch } from '@/api/authFetch'
import { server } from '@/test/server'
import { currentLocale, setLocale } from './index'
import { useFormat } from './useFormat'
import arCommon from './ar/common.json'

afterEach(async () => {
  await setLocale('en-US')
})

it('switching to Arabic sets lang and dir on html', async () => {
  await setLocale('ar')
  expect(currentLocale()).toBe('ar')
  expect(document.documentElement.lang).toBe('ar')
  expect(document.documentElement.dir).toBe('rtl')
  await setLocale('en-US')
  expect(document.documentElement.lang).toBe('en-US')
  expect(document.documentElement.dir).toBe('ltr')
})

it('Arabic numbers use Arabic-Indic digits', async () => {
  await setLocale('ar')
  const { result } = renderHook(() => useFormat())
  const s = result.current.number(1234)
  expect(s).toContain('١٬٢٣٤')
  expect(s).not.toMatch(/[0-9]/)
})

it('Arabic dates use Arabic-Indic digits', async () => {
  await setLocale('ar')
  const { result } = renderHook(() => useFormat())
  expect(result.current.date('2026-10-05T12:00:00Z')).not.toMatch(/[0-9]/)
  expect(result.current.dateTime('2026-10-05T12:00:00Z')).not.toMatch(/[0-9]/)
  expect(result.current.relative('2026-10-05T11:55:00Z', new Date('2026-10-05T12:00:00Z'))).not.toMatch(/[0-9]/)
})

it('authFetch sends the on-screen language', async () => {
  await setLocale('ar')
  let seen: string | null = null
  server.use(
    http.get('http://api.test/x', ({ request }) => {
      seen = request.headers.get('accept-language')
      return HttpResponse.json({})
    }),
  )
  const f = createAuthFetch({ getAccessToken: () => 't', renew: () => Promise.resolve(null), signOut: () => undefined })
  await f(new Request('http://api.test/x'))
  expect(seen).toBe('ar')
})

it('unknown error code falls back to the generic message', async () => {
  await setLocale('ar')
  const m = messageFor(new ApiError(500, 'future_code', 'English detail'))
  expect(m).toBe(arCommon.errors.generic)
  expect(m).not.toContain('English detail')
})
