import 'i18next'
import type audit from './en-US/audit.json'
import type common from './en-US/common.json'
import type home from './en-US/home.json'
import type jobs from './en-US/jobs.json'
import type me from './en-US/me.json'
import type users from './en-US/users.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: {
      common: typeof common
      me: typeof me
      home: typeof home
      users: typeof users
      audit: typeof audit
      jobs: typeof jobs
    }
  }
}
