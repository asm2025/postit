import { clearPending, loadPending, PENDING_KEY, savePending, type PendingEntry } from './pendingStore'

const NOW = Date.parse('2026-11-27T12:00:00Z')
const entry: PendingEntry = {
  connectId: 'secret-connect-id',
  pendingId: 'pending-1',
  platform: 'p1',
  userId: 'user-1',
  expiresAt: '2026-11-27T12:10:00Z',
}

afterEach(() => {
  sessionStorage.clear()
  localStorage.clear()
})

it('reads back what it saved, for the same user inside the ten minutes', () => {
  savePending(entry)
  expect(loadPending('user-1', NOW)).toEqual(entry)
})

it('expired entry is dropped', () => {
  savePending(entry)
  expect(loadPending('user-1', Date.parse('2026-11-27T12:10:01Z'))).toBeNull()
  expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
})

it("another user's entry is ignored and dropped", () => {
  savePending(entry)
  expect(loadPending('user-2', NOW)).toBeNull()
  expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
})

it('a damaged entry is dropped', () => {
  sessionStorage.setItem(PENDING_KEY, '{not json')
  expect(loadPending('user-1', NOW)).toBeNull()
  sessionStorage.setItem(PENDING_KEY, JSON.stringify({ connectId: 1 }))
  expect(loadPending('user-1', NOW)).toBeNull()
  expect(sessionStorage.getItem(PENDING_KEY)).toBeNull()
})

it('keeps a credential check with no row to cancel', () => {
  savePending({ ...entry, pendingId: null })
  expect(loadPending('user-1', NOW)?.pendingId).toBeNull()
})

it('clear removes it', () => {
  savePending(entry)
  clearPending()
  expect(loadPending('user-1', NOW)).toBeNull()
})

it('never touches localStorage', () => {
  const set = vi.spyOn(Storage.prototype, 'setItem')
  savePending(entry)
  expect(localStorage.length).toBe(0)
  const writes = set.mock.instances.filter((s) => s === localStorage)
  expect(writes).toHaveLength(0)
  set.mockRestore()
})
