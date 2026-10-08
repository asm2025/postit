import { greeting, initials, relativeTime } from './format'

describe('relativeTime', () => {
  const now = new Date('2026-10-05T12:00:00Z')
  it('formats minutes, hours and days', () => {
    expect(relativeTime('2026-10-05T11:55:00Z', now)).toBe('5 minutes ago')
    expect(relativeTime('2026-10-05T09:00:00Z', now)).toBe('3 hours ago')
    expect(relativeTime('2026-10-03T12:00:00Z', now)).toBe('2 days ago')
    expect(relativeTime('2026-10-05T11:59:50Z', now)).toBe('just now')
  })
})

describe('initials', () => {
  it('uses first and last word, falls back for blanks', () => {
    expect(initials('Postit Admin')).toBe('PA')
    expect(initials('  ada   king lovelace ')).toBe('AL')
    expect(initials('root')).toBe('RO')
    expect(initials('   ')).toBe('?')
  })
})

describe('greeting', () => {
  it('follows the time of day', () => {
    expect(greeting(new Date(2026, 9, 5, 3))).toBe('Working late')
    expect(greeting(new Date(2026, 9, 5, 9))).toBe('Good morning')
    expect(greeting(new Date(2026, 9, 5, 14))).toBe('Good afternoon')
    expect(greeting(new Date(2026, 9, 5, 21))).toBe('Good evening')
  })
})
