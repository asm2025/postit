import i18next from 'i18next'
import { formatLocale, type AppLocale } from '@/i18n/locales'

export function relativeTime(iso: string, locale: AppLocale, now: Date = new Date()): string {
  const seconds = Math.round((new Date(iso).getTime() - now.getTime()) / 1000)
  const abs = Math.abs(seconds)
  if (abs < 60) return i18next.t('format.justNow', { lng: locale })
  const rtf = new Intl.RelativeTimeFormat(formatLocale(locale), { numeric: 'always' })
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(seconds / 3600), 'hour')
  return rtf.format(Math.round(seconds / 86400), 'day')
}

/** Up to two initials for the avatar tile; never empty. */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  const first = (w: string) => Array.from(w)[0] ?? ''
  const letters =
    parts.length === 1 ? Array.from(parts[0]).slice(0, 2) : [first(parts[0]), first(parts[parts.length - 1])]
  return letters.join('').toUpperCase()
}

export function greeting(locale: AppLocale, now: Date = new Date()): string {
  const h = now.getHours()
  const key =
    h < 5
      ? 'format.greeting.workingLate'
      : h < 12
        ? 'format.greeting.morning'
        : h < 18
          ? 'format.greeting.afternoon'
          : 'format.greeting.evening'
  return i18next.t(key, { lng: locale })
}

export function dateLabel(locale: AppLocale, now: Date = new Date()): string {
  try {
    return now.toLocaleDateString(formatLocale(locale), { weekday: 'long', day: 'numeric', month: 'long' })
  } catch {
    return ''
  }
}
