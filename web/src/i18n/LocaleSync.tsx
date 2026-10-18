import { useQuery } from '@tanstack/react-query'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import { useSession } from '@/auth/session'
import { applyDocumentLocale, currentLocale, localeUnsaved, setLocale, setLocaleUnsaved } from './index'
import { toAppLocale } from './locales'

/**
 * Keeps `<html lang dir>` in step with the language. After sign-in, a language picked while signed out
 * is saved to the account; otherwise the account's saved language is adopted.
 */
export function LocaleSync() {
  const { i18n } = useTranslation()
  const api = useApi()
  const { status } = useSession()

  useEffect(() => {
    applyDocumentLocale()
  }, [i18n.language])

  const prefs = useQuery({
    queryKey: ['preferences'],
    queryFn: async () => {
      if (!localeUnsaved()) return unwrap(api.GET('/api/v1/me/preferences'))
      const saved = await unwrap(api.PUT('/api/v1/me/preferences', { body: { locale: currentLocale() } }))
      setLocaleUnsaved(false)
      return saved
    },
    enabled: status === 'signedIn',
    staleTime: Infinity,
    retry: false,
  })
  const stored = prefs.data?.locale
  useEffect(() => {
    if (stored) void setLocale(toAppLocale(stored))
  }, [stored])

  return null
}
