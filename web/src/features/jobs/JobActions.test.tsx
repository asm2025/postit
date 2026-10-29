import { screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { setLocale } from '@/i18n'
import arJobs from '@/i18n/ar/jobs.json'
import enJobs from '@/i18n/en-US/jobs.json'
import { renderApp } from '@/test/render'
import { JobActions } from './JobActions'
import type { Job } from './queries'

const CODES = {
  already_retried: 'retried',
  type_not_registered: 'unknownType',
  must_finish: 'deleteUser',
  publishing_managed: 'publishingManaged',
} as const

function job(code: string): Job {
  return {
    id: '019a0000-0000-7000-8000-000000000001',
    actions: { retry: false, cancel: false, delete: false },
    actions_disabled_reason: code,
  } as unknown as Job
}

afterEach(async () => {
  await setLocale('en-US')
})

describe.each([
  ['en-US', enJobs],
  ['ar', arJobs],
] as const)('%s', (locale, catalog) => {
  it.each(Object.entries(CODES))('translates the %s code', async (code, key) => {
    await setLocale(locale)
    renderApp(<JobActions job={job(code)} />)
    expect(await screen.findByText(catalog.actions.reason[key])).toBeInTheDocument()
  })

  it('falls back to the generic line for an unknown code', async () => {
    await setLocale(locale)
    renderApp(<JobActions job={job('something_new')} />)
    expect(await screen.findByText(catalog.actions.reason.generic)).toBeInTheDocument()
  })
})
