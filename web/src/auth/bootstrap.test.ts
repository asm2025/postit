import { isSilentFrame } from './bootstrap'

const top = {}
const frame = { self: {}, top, location: { pathname: '/auth/callback' } }

it('is a silent frame only when framed on the callback route', () => {
  expect(isSilentFrame(frame)).toBe(true)
  expect(isSilentFrame({ ...frame, location: { pathname: '/' } })).toBe(false)
  const self = {}
  expect(isSilentFrame({ self, top: self, location: { pathname: '/auth/callback' } })).toBe(false)
})
