import { http, HttpResponse } from 'msw'
import { renderHook } from '@testing-library/react'
import { ApiError, messageFor } from '@/api/errors'
import { createAuthFetch } from '@/api/authFetch'
import { server } from '@/test/server'
import { currentLocale, setLocale } from './index'
import { dirOf, SUPPORTED, toAppLocale } from './locales'
import { useFormat } from './useFormat'
import enCommon from './en-US/common.json'

afterEach(async () => {
  await setLocale('en-US')
})

it('setting a language sets lang and dir on html from its direction', async () => {
  for (const l of SUPPORTED) {
    await setLocale(l)
    expect(currentLocale()).toBe(l)
    expect(document.documentElement.lang).toBe(l)
    expect(document.documentElement.dir).toBe(dirOf(l))
  }
  expect(dirOf('en-US')).toBe('ltr')
})

it('maps any tag to a supported language by its primary subtag', () => {
  expect(toAppLocale('en-GB')).toBe('en-US')
  expect(toAppLocale('EN')).toBe('en-US')
  expect(toAppLocale('fr-CA')).toBe('en-US')
  expect(toAppLocale(undefined)).toBe('en-US')
})

it('numbers use the language format', () => {
  const { result } = renderHook(() => useFormat())
  expect(result.current.number(1234)).toBe('1,234')
})

it('authFetch sends the on-screen language', async () => {
  let seen: string | null = null
  server.use(
    http.get('http://api.test/x', ({ request }) => {
      seen = request.headers.get('accept-language')
      return HttpResponse.json({})
    }),
  )
  const f = createAuthFetch({ getAccessToken: () => 't', renew: () => Promise.resolve(null), signOut: () => undefined })
  await f(new Request('http://api.test/x'))
  expect(seen).toBe('en-US')
})

it('unknown error code falls back to the generic message', () => {
  const m = messageFor(new ApiError(500, 'future_code', 'English detail'))
  expect(m).toBe(enCommon.errors.generic)
  expect(m).not.toContain('English detail')
})
