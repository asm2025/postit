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
