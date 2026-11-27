/**
 * The connection a user left unfinished in this tab, so a reload inside its ten minutes shows the
 * checklist again. `sessionStorage` only, never `localStorage`: the `connect_id` is a one-time
 * secret that must not outlive the tab or reach another one.
 */
export const PENDING_KEY = 'postit.pendingConnection'

export interface PendingEntry {
  connectId: string
  /** The row to cancel the check by; null for an OAuth sign-in, which is discarded locally. */
  pendingId: string | null
  platform: string
  userId: string
  /** RFC 3339: when the connection stops working. */
  expiresAt: string
}

export function savePending(entry: PendingEntry): void {
  try {
    sessionStorage.setItem(PENDING_KEY, JSON.stringify(entry))
  } catch {
    // Storage blocked: the checklist works, it just does not survive a reload.
  }
}

export function clearPending(): void {
  try {
    sessionStorage.removeItem(PENDING_KEY)
  } catch {
    // Nothing stored that could be read back.
  }
}

/** The entry still good for `userId`; an expired one, or another user's, is dropped. */
export function loadPending(userId: string, now: number = Date.now()): PendingEntry | null {
  let raw: string | null
  try {
    raw = sessionStorage.getItem(PENDING_KEY)
  } catch {
    return null
  }
  if (raw === null) return null
  try {
    const entry = JSON.parse(raw) as Partial<PendingEntry>
    const { connectId, pendingId, platform, expiresAt } = entry
    if (
      typeof connectId === 'string' &&
      typeof platform === 'string' &&
      typeof expiresAt === 'string' &&
      (pendingId === null || typeof pendingId === 'string') &&
      entry.userId === userId &&
      Date.parse(expiresAt) > now
    ) {
      return { connectId, pendingId, platform, userId, expiresAt }
    }
  } catch {
    // Not ours: fall through and drop it.
  }
  clearPending()
  return null
}
