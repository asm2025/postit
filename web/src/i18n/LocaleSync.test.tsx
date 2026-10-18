import { waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { currentLocale, localeUnsaved, setLocale, setLocaleUnsaved } from '@/i18n'
import { server } from '@/test/server'
import { API } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { LocaleSync } from './LocaleSync'

afterEach(async () => {
  setLocaleUnsaved(false)
  await setLocale('en-US')
})

it('adopts the account language after sign-in', async () => {
  server.use(http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json({ locale: 'ar' })))
  renderApp(<LocaleSync />)
  await waitFor(() => {
    expect(currentLocale()).toBe('ar')
  })
})

it('saves a language picked while signed out instead of reading the account', async () => {
  const saved = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json({ locale: 'en-US' })),
    http.put(`${API}/api/v1/me/preferences`, async ({ request }) => {
      saved(await request.json())
      return HttpResponse.json({ locale: 'ar' })
    }),
  )
  await setLocale('ar')
  setLocaleUnsaved(true)
  renderApp(<LocaleSync />)
  await waitFor(() => {
    expect(saved).toHaveBeenCalledWith({ locale: 'ar' })
  })
  expect(currentLocale()).toBe('ar')
  expect(localeUnsaved()).toBe(false)
})
