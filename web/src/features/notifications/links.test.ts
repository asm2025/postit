import { readFileSync } from 'node:fs'
import path from 'node:path'
import { NOTIFICATION_KINDS } from '@/features/me/notificationKinds'
import enNotifications from '@/i18n/en-US/notifications.json'
import { notificationFixture } from '@/test/fixtures'
import { linkFor } from './links'

const ME = '00000000-0000-7000-8000-000000000001'
const OTHER = '00000000-0000-7000-8000-000000000002'
const ACCOUNT = '018f0000-0000-7000-8000-0000000000ac'

// Kinds D4a leaves without a page; D4b and later fill them in.
const NO_PAGE_YET = [
  'delivery_dead_lettered',
  'delivery_failed',
  'delivery_outcome_unknown',
  'delivery_parked',
  'delivery_stale',
  'delivery_published',
  'job_partial',
  'recurrence_skipped',
  'new_comments',
]

it('linkFor covers every kind', () => {
  expect(NOTIFICATION_KINDS.length).toBeGreaterThan(0)
  for (const kind of NOTIFICATION_KINDS) {
    const link = linkFor(notificationFixture(kind), ME)
    expect(link === null, `${kind} is ${link ? 'linked' : 'unlinked'}`).toBe(NO_PAGE_YET.includes(kind))
  }
})

it('sends an account notice to the account, in the notice owner workspace', () => {
  for (const kind of [
    'account_needs_reconnect',
    'account_revoked_by_platform',
    'reconnect_by_passed',
    'reconnect_reminder',
  ] as const) {
    expect(linkFor(notificationFixture(kind, { owner_id: OTHER }), ME)).toEqual({
      path: `/accounts/${ACCOUNT}`,
      scope: 'owner',
    })
  }
})

it('a notice with no owner is always about the user', () => {
  const link = linkFor(notificationFixture('account_needs_reconnect', { owner_id: null }), ME)
  expect(link?.scope).toBe('self')
})

it('delegation_changed leads to granted when the grant is mine, to received otherwise', () => {
  expect(linkFor(notificationFixture('delegation_changed', { owner_id: ME }), ME)).toEqual({
    path: '/delegations?tab=granted',
    scope: 'self',
  })
  expect(linkFor(notificationFixture('delegation_changed', { owner_id: OTHER }), ME)).toEqual({
    path: '/delegations?tab=received',
    scope: 'self',
  })
})

it('user_pending leads to the users page', () => {
  expect(linkFor(notificationFixture('user_pending', { owner_id: null }), ME)).toEqual({
    path: '/users',
    scope: 'self',
  })
})

describe('the kind catalog', () => {
  const spec = JSON.parse(readFileSync(path.join(import.meta.dirname, '../../../../backend/openapi.json'), 'utf8')) as {
    components: { schemas: { NotificationKind: { enum: string[] } } }
  }
  const kinds = spec.components.schemas.NotificationKind.enum

  it('has a label for every notification kind', () => {
    expect(kinds.length).toBeGreaterThan(0)
    for (const kind of kinds) {
      const label = (enNotifications.kind as Record<string, string | undefined>)[kind]
      expect(label?.trim(), `notifications:kind.${kind}`).toBeTruthy()
    }
  })

  it('has no label for a kind the API does not send', () => {
    for (const key of Object.keys(enNotifications.kind)) expect(kinds, key).toContain(key)
  })

  it('knows every kind the API sends', () => {
    expect([...NOTIFICATION_KINDS].sort()).toEqual([...kinds].sort())
  })
})
