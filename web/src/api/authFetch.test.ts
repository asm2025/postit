import { vi } from 'vitest'
import { createAuthFetch, type TokenSource } from './authFetch'

function source(over: Partial<TokenSource> = {}): TokenSource {
  return {
    getAccessToken: () => 'old',
    renew: vi.fn(() => Promise.resolve<string | null>('new')),
    signOut: vi.fn(),
    ...over,
  }
}
const req = (body?: string) => new Request('http://api.test/x', { method: body ? 'POST' : 'GET', body })

it('attaches the bearer token', async () => {
  const base = vi.fn((r: Request) => Promise.resolve(new Response(r.headers.get('authorization'))))
  const res = await createAuthFetch(source(), base)(req())
  expect(await res.text()).toBe('Bearer old')
})

it('renews once on 401 and retries with the new token, including the body', async () => {
  const seen: string[] = []
  const base = vi.fn(async (r: Request) => {
    seen.push(`${r.headers.get('authorization') ?? ''}|${await r.text()}`)
    return new Response(null, { status: seen.length === 1 ? 401 : 200 })
  })
  const s = source()
  const res = await createAuthFetch(s, base)(req('payload'))
  expect(res.status).toBe(200)
  expect(seen).toEqual(['Bearer old|payload', 'Bearer new|payload'])
  expect(s.renew).toHaveBeenCalledTimes(1)
  expect(s.signOut).not.toHaveBeenCalled()
})

it('signs out and returns the 401 when renewal fails', async () => {
  const base = vi.fn(() => Promise.resolve(new Response(null, { status: 401 })))
  const s = source({ renew: vi.fn(() => Promise.resolve(null)) })
  const res = await createAuthFetch(s, base)(req())
  expect(res.status).toBe(401)
  expect(s.signOut).toHaveBeenCalledTimes(1)
  expect(base).toHaveBeenCalledTimes(1)
})

it('signs out when the retry is still 401, without looping', async () => {
  const base = vi.fn(() => Promise.resolve(new Response(null, { status: 401 })))
  const s = source()
  const res = await createAuthFetch(s, base)(req())
  expect(res.status).toBe(401)
  expect(base).toHaveBeenCalledTimes(2)
  expect(s.signOut).toHaveBeenCalledTimes(1)
})

it('concurrent 401s share one renewal when the source is single-flight', async () => {
  const { singleFlight } = await import('@/lib/singleFlight')
  const renew = vi.fn(() => Promise.resolve<string | null>('new'))
  const s = source({ renew: singleFlight(renew) })
  const base = vi.fn((r: Request) =>
    Promise.resolve(new Response(null, { status: r.headers.get('authorization') === 'Bearer old' ? 401 : 200 })),
  )
  const f = createAuthFetch(s, base)
  const [a, b] = await Promise.all([f(req()), f(req())])
  expect([a.status, b.status]).toEqual([200, 200])
  expect(renew).toHaveBeenCalledTimes(1)
})

describe('act-as', () => {
  const at = (path: string) => new Request(`http://api.test${path}`)
  const echo = (r: Request) => Promise.resolve(new Response(r.headers.get('x-postit-act-as') ?? 'none'))

  it('adds act-as only where needed', async () => {
    const fetchAs = createAuthFetch(source(), echo, () => 'owner-1')
    expect(await (await fetchAs(at('/api/v1/accounts'))).text()).toBe('owner-1')
    expect(await (await fetchAs(at('/api/v1/media/x'))).text()).toBe('owner-1')
    expect(await (await fetchAs(at('/api/v1/me'))).text()).toBe('none')
    expect(await (await fetchAs(at('/api/v1/users/lookup?email=a'))).text()).toBe('none')
    expect(await (await fetchAs(at('/api/v1/admin/platforms'))).text()).toBe('none')
  })

  it('no header in own workspace', async () => {
    const fetchAs = createAuthFetch(source(), echo, () => null)
    expect(await (await fetchAs(at('/api/v1/accounts'))).text()).toBe('none')
  })

  it('keeps the same owner on the retry after a renewal', async () => {
    let owner: string | null = 'owner-1'
    const sent: (string | null)[] = []
    const base = vi.fn((r: Request) => {
      sent.push(r.headers.get('x-postit-act-as'))
      owner = null // the tab switched while the first try was in flight
      return Promise.resolve(new Response(null, { status: sent.length === 1 ? 401 : 200 }))
    })
    await createAuthFetch(source(), base, () => owner)(at('/api/v1/accounts'))
    expect(sent).toEqual(['owner-1', 'owner-1'])
  })
})
