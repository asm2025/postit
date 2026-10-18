import 'i18next'
import type common from './en-US/common.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: { common: typeof common }
  }
}
