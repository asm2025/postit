import { waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { currentLocale, localeUnsaved, setLocale, setLocaleUnsaved } from '@/i18n'
import { server } from '@/test/server'
import { API, preferencesFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { LocaleSync } from './LocaleSync'

afterEach(async () => {
  setLocaleUnsaved(false)
  await setLocale('en-US')
})

it('adopts the account language after sign-in', async () => {
  server.use(http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json(preferencesFixture({ locale: 'en-US' }))))
  renderApp(<LocaleSync />)
  await waitFor(() => {
    expect(currentLocale()).toBe('en-US')
  })
})

it('saves a language picked while signed out instead of reading the account', async () => {
  const saved = vi.fn()
  server.use(
    http.get(`${API}/api/v1/me/preferences`, () => HttpResponse.json(preferencesFixture())),
    http.put(`${API}/api/v1/me/preferences`, async ({ request }) => {
      saved(await request.json())
      return HttpResponse.json(preferencesFixture({ locale: 'en-US' }))
    }),
  )
  await setLocale('en-US')
  setLocaleUnsaved(true)
  renderApp(<LocaleSync />)
  await waitFor(() => {
    expect(saved).toHaveBeenCalledWith({ locale: 'en-US' })
  })
  expect(currentLocale()).toBe('en-US')
  expect(localeUnsaved()).toBe(false)
})
