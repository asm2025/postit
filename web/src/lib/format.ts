const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'always' })

export function relativeTime(iso: string, now: Date = new Date()): string {
  const seconds = Math.round((new Date(iso).getTime() - now.getTime()) / 1000)
  const abs = Math.abs(seconds)
  if (abs < 60) return 'just now'
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(seconds / 3600), 'hour')
  return rtf.format(Math.round(seconds / 86400), 'day')
}

/** Up to two initials for the avatar tile; never empty. */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  const first = (w: string) => Array.from(w)[0] ?? ''
  const letters =
    parts.length === 1 ? Array.from(parts[0]).slice(0, 2) : [first(parts[0]), first(parts[parts.length - 1])]
  return letters.join('').toUpperCase()
}

export function greeting(now: Date = new Date()): string {
  const h = now.getHours()
  if (h < 5) return 'Working late'
  if (h < 12) return 'Good morning'
  if (h < 18) return 'Good afternoon'
  return 'Good evening'
}

export function dateLabel(now: Date = new Date()): string {
  try {
    return now.toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long' })
  } catch {
    return ''
  }
}
