export type AppLocale = 'en-US' | 'ar'

export const SUPPORTED: readonly AppLocale[] = ['en-US', 'ar']

/** Maps any BCP 47 tag to a supported locale: `ar*` is Arabic, everything else English (US). */
export function toAppLocale(tag: string | null | undefined): AppLocale {
  return tag && /^ar(?:$|[-_])/i.test(tag.trim()) ? 'ar' : 'en-US'
}

/** The `Intl` locale; Arabic forces Arabic-Indic digits whatever the browser defaults to. */
export function formatLocale(l: AppLocale): string {
  return l === 'ar' ? 'ar-u-nu-arab' : 'en-US'
}

export function dirOf(l: AppLocale): 'rtl' | 'ltr' {
  return l === 'ar' ? 'rtl' : 'ltr'
}
