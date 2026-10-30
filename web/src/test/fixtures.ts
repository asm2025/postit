import type { components } from '@/api/schema'

export const API = 'http://api.test'

export type User = components['schemas']['UserDto']
export type Me = components['schemas']['MeDto']

export function userFixture(over: Partial<User> = {}): User {
  return {
    id: '00000000-0000-7000-8000-000000000001',
    email: 'user@postit.com',
    email_verified: true,
    display_name: 'Test User',
    role: 'member',
    status: 'active',
    approved_at: null,
    approved_by: null,
    last_seen_at: null,
    created_at: '2026-10-01T10:00:00Z',
    updated_at: '2026-10-01T10:00:00Z',
    ...over,
  }
}

export function meFixture(over: Partial<Me> = {}): Me {
  return { ...userFixture(), account_url: null, ...over }
}

export type Preferences = components['schemas']['PreferencesDto']

/** Every notification kind with its switches, critical kinds first; `email` is null where a kind is never emailed. */
export function preferencesFixture(over: Partial<Preferences> = {}): Preferences {
  const email = (kind: Preferences['switches'][number]['kind']) => ({ kind, push: true, email: true })
  return {
    locale: 'en-US',
    timezone: 'UTC',
    email_notifications: true,
    comment_digest: false,
    switches: [
      email('delivery_failed'),
      email('job_partial'),
      { kind: 'delivery_published', push: false, email: null },
      { kind: 'new_comments', push: true, email: null },
    ],
    ...over,
  }
}
