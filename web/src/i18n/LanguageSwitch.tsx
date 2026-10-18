import { useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApi } from '@/api/ApiContext'
import { ensureOk } from '@/api/client'
import { messageFor } from '@/api/errors'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { currentLocale, setLocale, setLocaleUnsaved } from './index'
import { SUPPORTED, toAppLocale, type AppLocale } from './locales'

// Each language is named in itself, whatever the screen language is.
const NAMES: Record<AppLocale, string> = { 'en-US': 'English', ar: 'العربية' }
const ITEMS = SUPPORTED.map((value) => ({ value, label: NAMES[value] }))

export function LanguageSwitch({ persist = false }: { persist?: boolean }) {
  const { t } = useTranslation()
  const api = useApi()
  const queryClient = useQueryClient()
  const [error, setError] = useState<string | null>(null)
  const locale = currentLocale()

  const change = async (next: AppLocale) => {
    const previous = currentLocale()
    if (next === previous) return
    setError(null)
    await setLocale(next)
    // Signed out, LocaleSync saves it to the account after sign-in.
    if (!persist) {
      setLocaleUnsaved(true)
      return
    }
    try {
      await ensureOk(api.PUT('/api/v1/me/preferences', { body: { locale: next } }))
      setLocaleUnsaved(false)
      queryClient.setQueryData(['preferences'], { locale: next })
    } catch (e) {
      // The screen must never disagree with the stored value.
      await setLocale(previous)
      setError(messageFor(e))
    }
  }

  return (
    <div className="flex flex-col gap-1">
      <Select
        value={locale}
        onValueChange={(v) => {
          void change(toAppLocale(v))
        }}
      >
        <SelectTrigger size="sm" aria-label={t('language.label')}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {ITEMS.map((i) => (
            <SelectItem key={i.value} value={i.value}>
              {i.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </div>
  )
}
