import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API, meFixture } from '@/test/fixtures'
import { STORE_KEY, stored, stubPush, type PushEnv } from '@/test/push'
import { renderApp } from '@/test/render'
import { useMe } from './useMe'
import { useSignOut } from './useSignOut'

const ID = '018f0000-0000-7000-8000-0000000000e1'

/** Loads the profile first, as the app shell does, so sign-out knows the signed-in user. */
function Button() {
  const me = useMe()
  const signOut = useSignOut(100)
  if (!me.data) return null
  return (
    <button type="button" onClick={signOut}>
      out
    </button>
  )
}

let env: PushEnv | null = null
beforeEach(() => {
  server.use(http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())))
})
afterEach(() => {
  env?.restore()
  env = null
})

function setup(respond: () => Response | Promise<Response>, deleted = vi.fn()) {
  env = stubPush({ permission: 'granted', existing: true })
  localStorage.setItem(STORE_KEY, stored(ID))
  const order: string[] = []
  server.use(
    http.delete(`${API}/api/v1/me/push-endpoints/:id`, ({ params }) => {
      deleted(params.id)
      order.push('delete')
      return respond()
    }),
  )
  const signOut = vi.fn(() => order.push('signOut'))
  env.subscription.unsubscribe.mockImplementation(() => {
    order.push('unsubscribe')
    return Promise.resolve(true)
  })
  return { signOut, order }
}

it('drops this browser push subscription before signing out', async () => {
  const deleted = vi.fn()
  const { signOut, order } = setup(() => new HttpResponse(null, { status: 204 }), deleted)
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(await screen.findByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
  expect(deleted).toHaveBeenCalledWith(ID)
  expect(order).toEqual(['delete', 'unsubscribe', 'signOut'])
  expect(JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}')).toEqual({})
})

it('signs out even when the server refuses the delete', async () => {
  const { signOut } = setup(() => HttpResponse.json({ code: 'internal', status: 500, title: 'x' }, { status: 500 }))
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(await screen.findByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
  expect(env?.subscription.unsubscribe).toHaveBeenCalled()
})

it('signs out when the delete hangs', async () => {
  const { signOut } = setup(async () => {
    await delay('infinite')
    return new HttpResponse(null, { status: 204 })
  })
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(await screen.findByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
})

it('signs out at once in a browser without push', async () => {
  const signOut = vi.fn()
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(await screen.findByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
})

it('never deletes another account registration on sign-out', async () => {
  const deleted = vi.fn()
  const { signOut } = setup(() => new HttpResponse(null, { status: 204 }), deleted)
  localStorage.setItem(STORE_KEY, stored(ID, '00000000-0000-7000-8000-000000000099'))
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(await screen.findByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
  expect(deleted).not.toHaveBeenCalled()
  expect(env?.subscription.unsubscribe).toHaveBeenCalled()
})
