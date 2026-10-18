import 'i18next'
import type common from './en-US/common.json'
import type home from './en-US/home.json'
import type me from './en-US/me.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: { common: typeof common; me: typeof me; home: typeof home }
  }
}
