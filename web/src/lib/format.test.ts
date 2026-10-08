import { relativeTime } from './format'

describe('relativeTime', () => {
  const now = new Date('2026-10-05T12:00:00Z')
  it('formats minutes, hours and days', () => {
    expect(relativeTime('2026-10-05T11:55:00Z', now)).toBe('5 minutes ago')
    expect(relativeTime('2026-10-05T09:00:00Z', now)).toBe('3 hours ago')
    expect(relativeTime('2026-10-03T12:00:00Z', now)).toBe('2 days ago')
    expect(relativeTime('2026-10-05T11:59:50Z', now)).toBe('just now')
  })
})
