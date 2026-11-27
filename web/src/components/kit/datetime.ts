const pad = (n: number) => String(n).padStart(2, '0')

/** An RFC 3339 instant as the `datetime-local` value of the browser's time zone, or '' when it is not one. */
export function toLocalInput(rfc: string): string {
  const d = new Date(rfc)
  if (Number.isNaN(d.getTime())) return ''
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** A `datetime-local` value, read in the browser's time zone, as RFC 3339 with that zone's offset. */
export function fromLocalInput(local: string): string {
  const d = new Date(local)
  if (Number.isNaN(d.getTime())) return ''
  const offset = -d.getTimezoneOffset()
  const sign = offset < 0 ? '-' : '+'
  const abs = Math.abs(offset)
  return `${local.slice(0, 16)}:00${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
}
