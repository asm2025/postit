/** What the interface needs to know about a language it ships in. */
interface LocaleInfo {
  /** The primary language subtag a browser or account tag is matched on. */
  language: string
  /** The `Intl` locale numbers and dates are formatted in. */
  intl: string
  /** The direction the script runs in; `<html dir>` and every mirrored layout follow it. */
  dir: 'ltr' | 'rtl'
}

const LOCALES = {
  'en-US': { language: 'en', intl: 'en-US', dir: 'ltr' },
} as const satisfies Record<string, LocaleInfo>

export type AppLocale = keyof typeof LOCALES

export const SUPPORTED = Object.keys(LOCALES) as readonly AppLocale[]

/** Whether there is more than one language to choose from; the language switch shows only then. */
export const MULTILINGUAL = SUPPORTED.length > 1

/** Maps any BCP 47 tag to a supported locale by its primary language subtag; otherwise English (US). */
export function toAppLocale(tag: string | null | undefined): AppLocale {
  const language = (tag ?? '').trim().split(/[-_]/)[0].toLowerCase()
  return SUPPORTED.find((l) => LOCALES[l].language === language) ?? 'en-US'
}

/** The `Intl` locale for numbers and dates. */
export function formatLocale(l: AppLocale): string {
  return LOCALES[l].intl
}

export function dirOf(l: AppLocale): 'rtl' | 'ltr' {
  return (LOCALES[l] as LocaleInfo).dir
}
