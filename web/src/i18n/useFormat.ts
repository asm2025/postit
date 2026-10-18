import { useTranslation } from 'react-i18next'
import { dateLabel, greeting, relativeTime } from '@/lib/format'
import { currentLocale } from './index'
import { formatLocale } from './locales'

/** Locale-aware formatting for the on-screen language; re-renders on a language change. */
export function useFormat() {
  useTranslation()
  const locale = currentLocale()
  const tag = formatLocale(locale)
  return {
    locale,
    number: (n: number): string => new Intl.NumberFormat(tag).format(n),
    date: (iso: string, opts: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }): string =>
      new Intl.DateTimeFormat(tag, opts).format(new Date(iso)),
    dateTime: (iso: string): string =>
      new Intl.DateTimeFormat(tag, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(iso)),
    relative: (iso: string, now?: Date): string => relativeTime(iso, locale, now),
    greeting: (now?: Date): string => greeting(locale, now),
    dateLabel: (now?: Date): string => dateLabel(locale, now),
  }
}
