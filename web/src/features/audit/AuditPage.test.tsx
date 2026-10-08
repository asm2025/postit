import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { server } from '@/test/server'
import { API } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { AuditPage } from './AuditPage'

const ACTOR = '0192a000-0000-7000-8000-00000000000a'
const SUBJECT = '0192a000-0000-7000-8000-00000000000b'
const PSEUDONYM = '0192a000-0000-8000-8000-00000000000c'

const events = [
  {
    id: 'e1',
    at: '2026-10-05T11:00:00Z',
    kind: 'user_approved',
    actor: { id: ACTOR, deleted: false, display_name: 'Ann Admin' },
    subject: { id: SUBJECT, deleted: false, display_name: null },
    owner: null,
    ip: null,
    request_id: null,
    details: {},
  },
  {
    id: 'e2',
    at: '2026-10-05T10:00:00Z',
    kind: 'future_kind',
    actor: null,
    subject: { id: PSEUDONYM, deleted: true, display_name: null },
    owner: null,
    ip: null,
    request_id: null,
    details: {},
  },
]

function list(spy = vi.fn()) {
  server.use(
    http.get(`${API}/api/v1/admin/audit`, ({ request }) => {
      spy(new URL(request.url).searchParams.toString())
      return HttpResponse.json({ data: events, page: 1, page_size: 20, total: events.length })
    }),
  )
  return spy
}

it('renders names, deleted users, short IDs and unknown kinds', async () => {
  list()
  renderApp(<AuditPage />, { route: '/audit' })
  // The kind filter's <option> also reads "User approved", so wait on the row's link.
  expect(await screen.findByRole('link', { name: 'Ann Admin' })).toHaveAttribute('href', `/audit?actor=${ACTOR}`)
  expect(screen.getAllByText('User approved')).toHaveLength(2)
  expect(screen.getByRole('link', { name: SUBJECT.slice(0, 8) })).toBeInTheDocument()
  expect(screen.getByText(/deleted user/i)).toBeInTheDocument()
  expect(screen.getByText('future_kind')).toBeInTheDocument()
})

it('filters by kind through the URL', async () => {
  const spy = list()
  const user = userEvent.setup()
  renderApp(<AuditPage />, { route: '/audit' })
  await screen.findByRole('link', { name: 'Ann Admin' })
  await user.selectOptions(screen.getByLabelText(/kind/i), 'role_changed')
  await waitFor(() => {
    expect(spy).toHaveBeenLastCalledWith(expect.stringContaining('kind=role_changed'))
  })
})

it('reads separate actor and subject filters from the URL', async () => {
  const spy = list()
  renderApp(<AuditPage />, { route: `/audit?actor=${ACTOR}&subject=${SUBJECT}` })
  await screen.findByRole('link', { name: 'Ann Admin' })
  const sent = spy.mock.lastCall?.[0] as string
  expect(sent).toContain(`actor_user_id=${ACTOR}`)
  expect(sent).toContain(`subject_user_id=${SUBJECT}`)
})

it('shows an error when the request fails', async () => {
  server.use(
    http.get(`${API}/api/v1/admin/audit`, () =>
      HttpResponse.json({ code: 'internal', status: 500, title: 'x' }, { status: 500 }),
    ),
  )
  renderApp(<AuditPage />, { route: '/audit' })
  expect(await screen.findByRole('alert')).toHaveTextContent(/went wrong on the server/i)
})
