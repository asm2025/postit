import { useQuery, useQueryClient } from '@tanstack/react-query'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { useEffect, useState } from 'react'
import { Outlet, Route, Routes, useLocation } from 'react-router'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import { API, delegatedWorkspaceFixture, ownWorkspaceFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { useWsKey } from './keys'
import { ACT_AS_KEY, getActAs, setActAs } from './store'
import { useWorkspace } from './context'

const A = '0000000a-0000-7000-8000-00000000000a'
const B = '0000000b-0000-7000-8000-00000000000b'
const WORKSPACES = [
  ownWorkspaceFixture(),
  delegatedWorkspaceFixture(A, { delegation_id: '00000000-0000-7000-8000-0000000000a1' }),
  delegatedWorkspaceFixture(B, { delegation_id: '00000000-0000-7000-8000-0000000000b1' }),
]

/** `/accounts` answers per workspace, named after the `X-Postit-Act-As` header it arrived with. */
function accounts(delay: Record<string, number> = {}) {
  server.use(
    http.get(`${API}/api/v1/accounts`, async ({ request }) => {
      const owner = request.headers.get('x-postit-act-as') ?? 'own'
      await new Promise((r) => setTimeout(r, delay[owner] ?? 0))
      return HttpResponse.json({
        data: [{ id: `acct-${owner}`, display_name: `Accounts of ${owner.slice(0, 8)}` }],
        page: 1,
        page_size: 20,
        total: 1,
      })
    }),
  )
}

function Accounts() {
  const api = useApi()
  const key = useWsKey()
  const { actAs } = useWorkspace()
  const q = useQuery({
    queryKey: key('accounts'),
    queryFn: () => unwrap(api.GET('/api/v1/accounts')),
  })
  return (
    <div>
      <p>in: {actAs ?? 'own'}</p>
      {q.data?.data.map((a) => (
        <p key={a.id}>{a.display_name}</p>
      ))}
    </div>
  )
}

function Switches() {
  const { switchTo } = useWorkspace()
  return (
    <div>
      <button
        onClick={() => {
          switchTo(A)
        }}
      >
        to A
      </button>
      <button
        onClick={() => {
          switchTo(B)
        }}
      >
        to B
      </button>
      <button
        onClick={() => {
          switchTo(null)
        }}
      >
        to own
      </button>
    </div>
  )
}

/** The shell's contract: the routed page is keyed on the workspace. */
function Layout() {
  const { workspaceKey, endedNotice } = useWorkspace()
  const { pathname } = useLocation()
  return (
    <div>
      <Switches />
      <p>at: {pathname}</p>
      {endedNotice && <p role="alert">{endedNotice}</p>}
      <Outlet key={workspaceKey} />
    </div>
  )
}

function routes(page = <Accounts />) {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="*" element={page} />
      </Route>
    </Routes>
  )
}

it("switching drops the old workspace's queries and late answers", async () => {
  accounts({ [A]: 400 })
  renderApp(null, { routes: routes(), actAs: A, workspaces: WORKSPACES })
  expect(await screen.findByText('in: 0000000a-0000-7000-8000-00000000000a')).toBeInTheDocument()

  // A's answer is still on its way when the tab moves to B.
  await userEvent.click(screen.getByRole('button', { name: 'to B' }))
  expect(await screen.findByText('Accounts of 0000000b')).toBeInTheDocument()
  await new Promise((r) => setTimeout(r, 600))

  expect(screen.queryByText('Accounts of 0000000a')).not.toBeInTheDocument()
  expect(screen.getByText('Accounts of 0000000b')).toBeInTheDocument()
})

it("A to B to A shows A's data", async () => {
  accounts()
  renderApp(null, { routes: routes(), actAs: A, workspaces: WORKSPACES })
  expect(await screen.findByText('Accounts of 0000000a')).toBeInTheDocument()

  await userEvent.click(screen.getByRole('button', { name: 'to B' }))
  expect(await screen.findByText('Accounts of 0000000b')).toBeInTheDocument()
  await userEvent.click(screen.getByRole('button', { name: 'to A' }))

  expect(await screen.findByText('Accounts of 0000000a')).toBeInTheDocument()
  expect(screen.queryByText('Accounts of 0000000b')).not.toBeInTheDocument()
})

it('switching remounts the page', async () => {
  accounts()
  function Draft() {
    const [text, setText] = useState('')
    return (
      <input
        aria-label="draft"
        value={text}
        onChange={(e) => {
          setText(e.target.value)
        }}
      />
    )
  }
  renderApp(null, { routes: routes(<Draft />), actAs: A, workspaces: WORKSPACES })
  await userEvent.type(await screen.findByLabelText('draft'), 'half a post')
  expect(screen.getByLabelText('draft')).toHaveValue('half a post')

  await userEvent.click(screen.getByRole('button', { name: 'to B' }))

  await waitFor(() => {
    expect(screen.getByLabelText('draft')).toHaveValue('')
  })
})

it('switching from a detail route goes home; from a list route stays', async () => {
  accounts()
  renderApp(null, { routes: routes(), route: '/accounts/some-id', actAs: A, workspaces: WORKSPACES })
  await userEvent.click(await screen.findByRole('button', { name: 'to B' }))
  expect(await screen.findByText('at: /')).toBeInTheDocument()
})

it('stays on a list page when the workspace switches', async () => {
  accounts()
  renderApp(null, { routes: routes(), route: '/accounts', actAs: A, workspaces: WORKSPACES })
  await userEvent.click(await screen.findByRole('button', { name: 'to B' }))
  expect(await screen.findByText('in: 0000000b-0000-7000-8000-00000000000b')).toBeInTheDocument()
  expect(screen.getByText('at: /accounts')).toBeInTheDocument()
})

it('an unscoped query key is dropped on switch too', async () => {
  const grabbed: { qc: ReturnType<typeof useQueryClient> | null } = { qc: null }
  function Grab() {
    const qc = useQueryClient()
    useEffect(() => {
      grabbed.qc = qc
    }, [qc])
    return <p>page</p>
  }
  renderApp(null, { routes: routes(<Grab />), actAs: A, workspaces: WORKSPACES })
  await screen.findByText('page')
  const cache = grabbed.qc
  if (!cache) throw new Error('no query client')
  cache.setQueryData(['accounts-unscoped'], 'old workspace data')
  cache.setQueryData(['me'], 'the user')

  await userEvent.click(screen.getByRole('button', { name: 'to B' }))

  await waitFor(() => {
    expect(cache.getQueryData(['accounts-unscoped'])).toBeUndefined()
  })
  // The user's own data is the same in every workspace and stays.
  expect(cache.getQueryData(['me'])).toBe('the user')
})

it('delegation_invalid returns to own workspace once', async () => {
  let mounts = 0
  function Failing() {
    const api = useApi()
    const key = useWsKey()
    useEffect(() => {
      mounts += 1
    }, [])
    useQuery({ queryKey: key('one'), queryFn: () => unwrap(api.GET('/api/v1/accounts')) })
    useQuery({ queryKey: key('two'), queryFn: () => unwrap(api.GET('/api/v1/target-groups')) })
    return <p>in: {useWorkspace().actAs ?? 'own'}</p>
  }
  const ended = () => HttpResponse.json({ code: 'delegation_invalid' }, { status: 403 })
  server.use(
    http.get(`${API}/api/v1/accounts`, ({ request }) =>
      request.headers.has('x-postit-act-as')
        ? ended()
        : HttpResponse.json({ data: [], page: 1, page_size: 20, total: 0 }),
    ),
    http.get(`${API}/api/v1/target-groups`, ({ request }) =>
      request.headers.has('x-postit-act-as')
        ? ended()
        : HttpResponse.json({ data: [], page: 1, page_size: 20, total: 0 }),
    ),
  )
  renderApp(null, { routes: routes(<Failing />), actAs: A, workspaces: WORKSPACES })

  expect(await screen.findByText('in: own')).toBeInTheDocument()
  expect(await screen.findByRole('alert')).toHaveTextContent(/access to that workspace ended/i)
  expect(getActAs()).toBeNull()
  // Mounted in A, then once in the own workspace; two failing queries made one switch.
  expect(mounts).toBe(2)
})

it('unknown stored owner falls back to own', async () => {
  accounts()
  renderApp(null, { routes: routes(), actAs: 'ghost', workspaces: [ownWorkspaceFixture()] })
  expect(await screen.findByText('in: own')).toBeInTheDocument()
  expect(getActAs()).toBeNull()
})

it('act-as survives a remount (sessionStorage)', async () => {
  accounts()
  setActAs(B)
  expect(sessionStorage.getItem(ACT_AS_KEY)).toBe(B)
  renderApp(null, { routes: routes(), workspaces: WORKSPACES })
  expect(await screen.findByText('Accounts of 0000000b')).toBeInTheDocument()
  expect(screen.getByText('in: 0000000b-0000-7000-8000-00000000000b')).toBeInTheDocument()
})
