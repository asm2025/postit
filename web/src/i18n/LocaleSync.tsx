import { useQuery } from '@tanstack/react-query'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import { useSession } from '@/auth/session'
import { applyDocumentLocale, setLocale } from './index'
import { toAppLocale } from './locales'

/** Keeps `<html lang dir>` in step with the language and adopts the saved language after sign-in. */
export function LocaleSync() {
  const { i18n } = useTranslation()
  const api = useApi()
  const { status } = useSession()

  useEffect(() => {
    applyDocumentLocale()
  }, [i18n.language])

  const prefs = useQuery({
    queryKey: ['preferences'],
    queryFn: () => unwrap(api.GET('/api/v1/me/preferences')),
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
