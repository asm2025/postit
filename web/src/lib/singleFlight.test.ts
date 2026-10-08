import { singleFlight } from './singleFlight'

it('shares one in-flight call and allows a new one afterwards', async () => {
  let calls = 0
  let release!: (v: string) => void
  const slow = singleFlight(
    () =>
      new Promise<string>((r) => {
        calls++
        release = r
      }),
  )
  const a = slow()
  const b = slow()
  release('x')
  expect(await a).toBe('x')
  expect(await b).toBe('x')
  expect(calls).toBe(1)
  const c = slow()
  release('y')
  expect(await c).toBe('y')
  expect(calls).toBe(2)
})
