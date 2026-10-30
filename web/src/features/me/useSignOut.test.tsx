import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API } from '@/test/fixtures'
import { ENDPOINT, STORE_KEY, stubPush, type PushEnv } from '@/test/push'
import { renderApp } from '@/test/render'
import { useSignOut } from './useSignOut'

const ID = '018f0000-0000-7000-8000-0000000000e1'

function Button() {
  const signOut = useSignOut(100)
  return (
    <button type="button" onClick={signOut}>
      out
    </button>
  )
}

let env: PushEnv | null = null
afterEach(() => {
  env?.restore()
  env = null
})

function setup(respond: () => Response | Promise<Response>, deleted = vi.fn()) {
  env = stubPush({ permission: 'granted', existing: true })
  localStorage.setItem(STORE_KEY, JSON.stringify({ [ENDPOINT]: ID }))
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
  await user.click(screen.getByRole('button', { name: 'out' }))
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
  await user.click(screen.getByRole('button', { name: 'out' }))
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
  await user.click(screen.getByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
})

it('signs out at once in a browser without push', async () => {
  const signOut = vi.fn()
  const user = userEvent.setup()
  renderApp(<Button />, { session: { signOut } })
  await user.click(screen.getByRole('button', { name: 'out' }))
  await waitFor(() => {
    expect(signOut).toHaveBeenCalledTimes(1)
  })
})
