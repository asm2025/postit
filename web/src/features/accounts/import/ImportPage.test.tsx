import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import enAccounts from '@/i18n/en-US/accounts.json'
import { API, meFixture, platformFixture } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { ImportPage } from './ImportPage'
import { IMPORT_STATUSES, IMPORT_STOPS } from './statuses'
import type { AccountImport } from './queries'

const ID = '018f0000-0000-7000-8000-0000000000b1'
const ID2 = '018f0000-0000-7000-8000-0000000000b2'

const fake = platformFixture('p1', 'Pixelfed', {
  connect: {
    method: 'credentials',
    schema: {
      type: 'object',
      properties: {
        host: { type: 'string', 'x-postit-label': { key: 'p1.host', en: 'Server' } },
        token: { type: 'string', 'x-postit-secret': true, 'x-postit-label': { key: 'p1.token', en: 'Token' } },
      },
      required: ['host', 'token'],
      additionalProperties: false,
    } as never,
  },
})
const oauthOnly = platformFixture('p2', 'Chirper', { connect: { method: 'oauth' } })

function dto(over: Partial<AccountImport> = {}): AccountImport {
  return {
    id: ID,
    platform: 'p1',
    commit: false,
    status: 'completed',
    rows_total: 2,
    rows_done: 2,
    stopped: null,
    last_row: 2,
    last_line: 3,
    counts: { ok: 1, invalid_fields: 1 },
    rows: [
      { row: 1, line: 2, status: 'ok', violations: [], field: null, account_ids: [], display_name_ignored: false },
      {
        row: 2,
        line: 3,
        status: 'invalid_fields',
        violations: ['option_required'],
        field: 'token',
        account_ids: [],
        display_name_ignored: false,
      },
    ],
    ...over,
  }
}

const csv = () => new File(['host,token\npix.example,tok-1\n'], 'accounts.csv', { type: 'text/csv' })

function mock(initial: { start?: AccountImport; get?: () => AccountImport } = {}) {
  const calls = {
    start: vi.fn(),
    startBody: vi.fn(),
    get: vi.fn(),
    cont: vi.fn(),
    cancel: vi.fn(),
  }
  server.use(
    http.get(`${API}/api/v1/me`, () => HttpResponse.json(meFixture())),
    http.get(`${API}/api/v1/platforms`, () => HttpResponse.json([fake, oauthOnly])),
    http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    http.post(`${API}/api/v1/accounts/import/p1`, async ({ request }) => {
      calls.start(new URL(request.url).searchParams.get('commit'), request.headers.get('content-type'))
      calls.startBody(await request.text())
      return HttpResponse.json(initial.start ?? dto(), { status: 202 })
    }),
    http.get(`${API}/api/v1/accounts/imports/:id`, () => {
      calls.get()
      return HttpResponse.json(initial.get ? initial.get() : dto())
    }),
    http.post(`${API}/api/v1/accounts/imports/:id/continue`, () => {
      calls.cont()
      return HttpResponse.json(dto({ commit: true, status: 'processing', rows_done: 0 }), { status: 202 })
    }),
    http.delete(`${API}/api/v1/accounts/imports/:id`, () => {
      calls.cancel()
      return HttpResponse.json(dto({ commit: true, status: 'canceled', rows: [], counts: {} }))
    }),
  )
  return calls
}

async function chooseAndCheck(user: ReturnType<typeof userEvent.setup>, file = csv()) {
  await user.click(await screen.findByRole('combobox', { name: 'Platform' }))
  await user.click(await screen.findByRole('option', { name: 'Pixelfed' }))
  await user.upload(screen.getByLabelText('CSV file'), file)
  await user.click(screen.getByRole('button', { name: 'Check file' }))
}

it('only credential platforms are offered', async () => {
  mock()
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await user.click(await screen.findByRole('combobox', { name: 'Platform' }))
  expect(await screen.findByRole('option', { name: 'Pixelfed' })).toBeInTheDocument()
  expect(screen.queryByRole('option', { name: 'Chirper' })).not.toBeInTheDocument()
})

it('lists the columns and offers a template', async () => {
  mock()
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await user.click(await screen.findByRole('combobox', { name: 'Platform' }))
  await user.click(await screen.findByRole('option', { name: 'Pixelfed' }))
  expect(await screen.findByText('host, token')).toBeInTheDocument()
  const link = screen.getByRole('link', { name: 'Download template' })
  expect(decodeURIComponent(link.getAttribute('href') ?? '')).toContain('host,token,display_name')
  expect(link).toHaveAttribute('download', 'p1-accounts.csv')
})

it('dry run shows progress then the report', async () => {
  let polls = 0
  const calls = mock({
    start: dto({ status: 'processing', rows_done: 0, rows: [], counts: {} }),
    get: () => (++polls < 2 ? dto({ status: 'processing', rows_done: 1, rows: [], counts: {} }) : dto()),
  })
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)

  expect(await screen.findByRole('progressbar', { name: 'Rows checked' })).toHaveAttribute('aria-valuemax', '2')
  const table = await screen.findByRole('table', { name: 'Report' }, { timeout: 8000 })
  const rows = within(table).getAllByRole('row').slice(1)
  expect(within(rows[0]).getByText('Ready to connect')).toBeInTheDocument()
  expect(within(rows[1]).getByText('Fields to fix')).toBeInTheDocument()
  expect(within(rows[1]).getByText('token')).toBeInTheDocument()
  expect(calls.start).toHaveBeenCalledWith('false', 'text/csv')
  expect(calls.startBody).toHaveBeenCalledWith('host,token\npix.example,tok-1\n')
  expect(screen.queryByRole('progressbar')).not.toBeInTheDocument()
}, 15_000)

it('polling stops when completed', async () => {
  const calls = mock({ start: dto() })
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)
  await screen.findByRole('table', { name: 'Report' })
  const seen = calls.get.mock.calls.length
  await new Promise((r) => setTimeout(r, 2600))
  expect(calls.get.mock.calls.length).toBe(seen)
}, 10_000)

it('commit resends the held file', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)
  await user.click(await screen.findByRole('button', { name: 'Connect valid rows' }))
  await waitFor(() => {
    expect(calls.start).toHaveBeenLastCalledWith('true', 'text/csv')
  })
  expect(calls.startBody).toHaveBeenLastCalledWith('host,token\npix.example,tok-1\n')
})

it('after reload asks to pick the file again', async () => {
  const calls = mock()
  const user = userEvent.setup()
  renderApp(<ImportPage />, { route: `/?import=${ID}` })
  await screen.findByRole('button', { name: 'Pick the file again to connect' })
  expect(screen.queryByRole('button', { name: 'Connect valid rows' })).not.toBeInTheDocument()
  expect(calls.start).not.toHaveBeenCalled()
  // The platform comes from the import itself.
  expect(screen.getByRole('combobox', { name: 'Platform' })).toBeDisabled()
  await user.upload(screen.getByLabelText('CSV file'), csv())
  expect(screen.queryByRole('button', { name: 'Pick the file again to connect' })).not.toBeInTheDocument()
  expect(await screen.findByRole('button', { name: 'Connect valid rows' })).toBeInTheDocument()
})

it('bad header answers at once', async () => {
  mock()
  server.use(
    http.post(`${API}/api/v1/accounts/import/p1`, () =>
      HttpResponse.json({ code: 'csv_invalid', status: 422, title: 'x' }, { status: 422 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)
  expect(await screen.findByText(/couldn't be read/)).toBeInTheDocument()
  expect(screen.queryByRole('table', { name: 'Report' })).not.toBeInTheDocument()
})

it('oversized declared file answers at once', async () => {
  mock()
  server.use(
    http.post(`${API}/api/v1/accounts/import/p1`, () =>
      HttpResponse.json({ code: 'payload_too_large', status: 413, title: 'x' }, { status: 413 }),
    ),
  )
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)
  expect(await screen.findByText('That is too large to send.')).toBeInTheDocument()
})

describe('a file that stopped early', () => {
  it.each(IMPORT_STOPS)('stopped import names the last line and the reason (%s)', async (stopped) => {
    mock({ start: dto({ stopped, last_row: 2, last_line: 3 }) })
    const user = userEvent.setup()
    renderApp(<ImportPage />)
    await chooseAndCheck(user)
    const reason = enAccounts.importStopped[stopped]
    expect(
      await screen.findByText(`The check stopped after line 3: ${reason}. The rows up to there are checked below.`),
    ).toBeInTheDocument()
  })

  const waiting = () =>
    dto({ commit: true, status: 'awaiting_decision', stopped: 'max_bytes', rows_done: 0, rows: [], counts: {} })

  it('stopped commit asks before connecting', async () => {
    const calls = mock({ get: waiting })
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    expect(
      await screen.findByText(
        'The file stopped after line 3: the file is larger than this server accepts. Nothing is connected yet.',
      ),
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Connect the 2 rows read' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Cancel import' })).toBeInTheDocument()
    expect(
      screen.getByText('If you do neither, this import ends after 24 hours with nothing connected.'),
    ).toBeInTheDocument()
    // Waiting for a person is not polled.
    const seen = calls.get.mock.calls.length
    await new Promise((r) => setTimeout(r, 2600))
    expect(calls.get.mock.calls.length).toBe(seen)
  }, 10_000)

  it('continue connects the rows read and polls to completion', async () => {
    let state: 'waiting' | 'processing' | 'done' = 'waiting'
    const calls = mock({
      get: () =>
        state === 'waiting'
          ? waiting()
          : state === 'processing'
            ? dto({ commit: true, status: 'processing', stopped: 'max_bytes', rows_done: 1, rows: [], counts: {} })
            : dto({ commit: true, stopped: 'max_bytes', last_line: 3 }),
    })
    server.use(
      http.post(`${API}/api/v1/accounts/imports/:id/continue`, () => {
        calls.cont()
        state = 'processing'
        setTimeout(() => {
          state = 'done'
        }, 300)
        return HttpResponse.json(
          dto({ commit: true, status: 'processing', stopped: 'max_bytes', rows_done: 0, rows: [], counts: {} }),
          {
            status: 202,
          },
        )
      }),
    )
    const user = userEvent.setup()
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    await user.click(await screen.findByRole('button', { name: 'Connect the 2 rows read' }))
    expect(await screen.findByRole('progressbar')).toBeInTheDocument()
    expect(
      await screen.findByText(
        'Remove lines 2 to 3 from your file and import it again to continue with the rest.',
        {},
        { timeout: 8000 },
      ),
    ).toBeInTheDocument()
    expect(calls.cont).toHaveBeenCalledOnce()
  }, 15_000)

  it('cancel asks first and shows canceled', async () => {
    const calls = mock({ get: waiting })
    const user = userEvent.setup()
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    await user.click(await screen.findByRole('button', { name: 'Cancel import' }))
    const confirm = await screen.findByRole('alertdialog')
    expect(calls.cancel).not.toHaveBeenCalled()
    await user.click(within(confirm).getByRole('button', { name: 'Cancel import' }))
    await waitFor(() => {
      expect(calls.cancel).toHaveBeenCalledOnce()
    })
    expect(await screen.findByText('Import canceled. Nothing was connected.')).toBeInTheDocument()
  })

  it('a waiting import survives a reload (import ID in the URL)', async () => {
    mock({ get: waiting })
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    expect(await screen.findByRole('button', { name: 'Connect the 2 rows read' })).toBeInTheDocument()
  })

  it('import_not_waiting reloads the state', async () => {
    let calls = 0
    mock({ get: () => (++calls === 1 ? waiting() : dto({ commit: true, status: 'canceled', rows: [], counts: {} })) })
    server.use(
      http.post(`${API}/api/v1/accounts/imports/:id/continue`, () =>
        HttpResponse.json({ code: 'import_not_waiting', status: 409, title: 'x' }, { status: 409 }),
      ),
    )
    const user = userEvent.setup()
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    await user.click(await screen.findByRole('button', { name: 'Connect the 2 rows read' }))
    expect(await screen.findByText('Import canceled. Nothing was connected.')).toBeInTheDocument()
  })

  it('failed says it did not finish', async () => {
    mock({ get: () => dto({ commit: true, status: 'failed', rows: [], counts: {} }) })
    renderApp(<ImportPage />, { route: `/?import=${ID}` })
    expect(await screen.findByText('This import did not finish.')).toBeInTheDocument()
  })
})

it('every import status has a label', () => {
  for (const status of IMPORT_STATUSES) {
    expect(enAccounts.importStatus[status].trim(), status).not.toBe('')
  }
  expect(Object.keys(enAccounts.importStatus).sort()).toEqual([...IMPORT_STATUSES].sort())
})

it('every stop reason has a label', () => {
  for (const stop of IMPORT_STOPS) expect(enAccounts.importStopped[stop].trim(), stop).not.toBe('')
  expect(Object.keys(enAccounts.importStopped).sort()).toEqual([...IMPORT_STOPS].sort())
})

it('a 1,000-row report renders and paginates', async () => {
  const rows = Array.from({ length: 1000 }, (_, i) => ({
    row: i + 1,
    line: i + 2,
    status: 'ok' as const,
    violations: [],
    field: null,
    account_ids: [],
    display_name_ignored: false,
  }))
  mock({
    get: () => dto({ rows, rows_total: 1000, rows_done: 1000, counts: { ok: 1000 }, last_row: 1000, last_line: 1001 }),
  })
  const user = userEvent.setup()
  renderApp(<ImportPage />, { route: `/?import=${ID}` })
  const table = await screen.findByRole('table', { name: 'Report' })
  expect(within(table).getAllByRole('row')).toHaveLength(21)
  expect(screen.getByText('Ready to connect: 1,000')).toBeInTheDocument()
  await user.click(screen.getByRole('button', { name: /next/i }))
  const second = within(await screen.findByRole('table', { name: 'Report' })).getAllByRole('row')[1]
  expect(within(second).getByText('22')).toBeInTheDocument()
})

it('malformed rows read in words', async () => {
  mock({
    get: () =>
      dto({
        rows: [
          {
            row: 1,
            line: 2,
            status: 'invalid_fields',
            violations: ['malformed_row'],
            field: null,
            account_ids: [],
            display_name_ignored: false,
          },
        ],
        counts: { invalid_fields: 1 },
      }),
  })
  renderApp(<ImportPage />, { route: `/?import=${ID}` })
  expect(await screen.findByText(enAccounts.importMalformed)).toBeInTheDocument()
})

it('the second import of a commit run is shown by its own id in the URL', async () => {
  mock({ start: dto({ id: ID2, commit: true, status: 'processing', rows_done: 0, rows: [], counts: {} }) })
  const user = userEvent.setup()
  renderApp(<ImportPage />)
  await chooseAndCheck(user)
  expect(await screen.findByRole('progressbar')).toBeInTheDocument()
})
