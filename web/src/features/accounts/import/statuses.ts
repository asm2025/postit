import type { ImportStatus, ImportStop } from './queries'

/** Every status a row can end in, in the order the counts read. A status the API adds fails typecheck here. */
const STATUS: Record<ImportStatus, true> = {
  ok: true,
  already_connected: true,
  invalid_fields: true,
  invalid_credentials: true,
  connected_elsewhere: true,
  nothing_found: true,
  platform_error: true,
}
export const IMPORT_STATUSES = Object.keys(STATUS) as ImportStatus[]

/** Every reason reading a file can stop early. */
const STOP: Record<ImportStop, true> = { max_rows: true, max_bytes: true, unreadable: true }
export const IMPORT_STOPS = Object.keys(STOP) as ImportStop[]
