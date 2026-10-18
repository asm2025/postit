import i18next from 'i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import { initReactI18next } from 'react-i18next'
import arCommon from './ar/common.json'
import arAudit from './ar/audit.json'
import arHome from './ar/home.json'
import arJobs from './ar/jobs.json'
import arMe from './ar/me.json'
import arUsers from './ar/users.json'
import enAudit from './en-US/audit.json'
import enCommon from './en-US/common.json'
import enHome from './en-US/home.json'
import enJobs from './en-US/jobs.json'
import enMe from './en-US/me.json'
import enUsers from './en-US/users.json'
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
        resources: {
          'en-US': { common: enCommon, me: enMe, home: enHome, users: enUsers, audit: enAudit, jobs: enJobs },
          ar: { common: arCommon, me: arMe, home: arHome, users: arUsers, audit: arAudit, jobs: arJobs },
        },
        defaultNS,
        ns: [defaultNS, 'me', 'home', 'users', 'audit', 'jobs'],
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
