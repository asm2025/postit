import i18next from 'i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import { initReactI18next } from 'react-i18next'
import arCommon from './ar/common.json'
import enCommon from './en-US/common.json'
import { dirOf, SUPPORTED, toAppLocale, type AppLocale } from './locales'

export const defaultNS = 'common'

export function currentLocale(): AppLocale {
  return toAppLocale(i18next.language)
}

/** Sets `<html lang dir>`. Done in JS because the CSP forbids an inline script. */
export function applyDocumentLocale(l: AppLocale = currentLocale()): void {
  document.documentElement.lang = l
  document.documentElement.dir = dirOf(l)
}

/** Awaited before the first render, so an Arabic visitor never sees a left-to-right flash. */
export async function initI18n(): Promise<void> {
  if (!i18next.isInitialized) {
    await i18next
      .use(LanguageDetector)
      .use(initReactI18next)
      .init({
        resources: { 'en-US': { common: enCommon }, ar: { common: arCommon } },
        defaultNS,
        ns: [defaultNS],
        fallbackLng: 'en-US',
        supportedLngs: [...SUPPORTED],
        nonExplicitSupportedLngs: false,
        load: 'currentOnly',
        interpolation: { escapeValue: false },
        detection: { order: ['localStorage', 'navigator'], caches: ['localStorage'] },
      })
  }
  applyDocumentLocale()
}

export async function setLocale(l: AppLocale): Promise<void> {
  await i18next.changeLanguage(l)
  applyDocumentLocale(l)
}
