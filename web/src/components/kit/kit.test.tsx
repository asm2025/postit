import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { vi } from 'vitest'
import { setLocale } from '@/i18n'
import { DangerZone } from './DangerZone'
import { DataTable } from './DataTable'
import { OptionSelect } from './OptionSelect'
import { TablePagination } from './TablePagination'
import { columnHelper } from './tableColumns'

interface Row {
  id: string
  name: string
  worker: string
}

const ROWS: Row[] = [
  { id: '1', name: 'Bravo', worker: 'w-2' },
  { id: '2', name: 'Alpha', worker: 'w-1' },
]
const col = columnHelper<Row>()
const COLUMNS = col.columns([
  col.accessor('name', { header: 'Name', enableSorting: true }),
  col.accessor('worker', { header: 'Worker', meta: { ltr: true } }),
])

const names = () =>
  screen
    .getAllByRole('row')
    .slice(1)
    .map((r) => within(r).getAllByRole('cell')[0]?.textContent)

describe('DataTable', () => {
  it('sorts only the columns that opt in, and says so', async () => {
    const user = userEvent.setup()
    render(<DataTable label="Rows" columns={COLUMNS} data={ROWS} getRowId={(r) => r.id} />)
    expect(screen.getByRole('table', { name: 'Rows' })).toBeInTheDocument()
    expect(within(screen.getByRole('columnheader', { name: 'Worker' })).queryByRole('button')).toBeNull()
    expect(names()).toEqual(['Bravo', 'Alpha'])
    await user.click(screen.getByRole('button', { name: 'Name' }))
    expect(names()).toEqual(['Alpha', 'Bravo'])
    expect(screen.getByRole('columnheader', { name: 'Name' })).toHaveAttribute('aria-sort', 'ascending')
  })

  it('keeps code-like values left-to-right', () => {
    render(<DataTable label="Rows" columns={COLUMNS} data={ROWS} />)
    expect(screen.getByText('w-1')).toHaveAttribute('dir', 'ltr')
  })

  it('shows the empty state instead of an empty table', () => {
    render(<DataTable label="Rows" columns={COLUMNS} data={[]} empty={<p>Nothing yet</p>} />)
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.getByText('Nothing yet')).toBeInTheDocument()
  })

  it('pages through the footer', async () => {
    const onPageChange = vi.fn()
    const user = userEvent.setup()
    render(
      <DataTable
        label="Rows"
        columns={COLUMNS}
        data={ROWS}
        pagination={{ page: 1, pageSize: 2, total: 5, onPageChange }}
      />,
    )
    await user.click(screen.getByRole('button', { name: 'Next page' }))
    expect(onPageChange).toHaveBeenCalledWith(2)
  })
})

describe('TablePagination', () => {
  afterEach(async () => {
    await setLocale('en-US')
  })

  it('shows the range and disables the ends', () => {
    render(<TablePagination page={1} pageSize={20} total={45} onPageChange={vi.fn()} />)
    expect(screen.getByText('1–20 of 45')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Previous page' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Next page' })).toBeEnabled()
  })

  it('jumps to a typed page, clamped to the last one', async () => {
    const onPageChange = vi.fn()
    const user = userEvent.setup()
    render(<TablePagination page={1} pageSize={20} total={45} onPageChange={onPageChange} />)
    const box = screen.getByRole('textbox', { name: /page number/i })
    await user.clear(box)
    await user.type(box, '9{Enter}')
    expect(onPageChange).toHaveBeenCalledWith(3)
  })

  it('changes the page size', async () => {
    const onPageSizeChange = vi.fn()
    const user = userEvent.setup()
    render(
      <TablePagination page={1} pageSize={20} total={45} onPageChange={vi.fn()} onPageSizeChange={onPageSizeChange} />,
    )
    await user.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    await user.click(await screen.findByRole('option', { name: '50' }))
    expect(onPageSizeChange).toHaveBeenCalledWith(50)
  })

  it('reads in Arabic with Arabic-Indic digits and mirrored chevrons', async () => {
    await setLocale('ar')
    render(<TablePagination page={2} pageSize={20} total={45} onPageChange={vi.fn()} />)
    expect(screen.getByText('٢١–٤٠ من ٤٥')).toBeInTheDocument()
    const next = screen.getByRole('button', { name: 'الصفحة التالية' })
    expect(next.querySelector('svg')).toHaveClass('rtl:-scale-x-100')
  })
})

describe('OptionSelect', () => {
  it('maps the "any" entry to an empty value', async () => {
    const onChange = vi.fn()
    const user = userEvent.setup()
    render(
      <OptionSelect
        aria-label="Status"
        value="active"
        anyLabel="Any status"
        options={[{ value: 'active', label: 'Active' }]}
        onChange={onChange}
      />,
    )
    await user.click(screen.getByRole('combobox', { name: 'Status' }))
    await user.click(await screen.findByRole('option', { name: 'Any status' }))
    expect(onChange).toHaveBeenCalledWith('')
  })
})

describe('DangerZone', () => {
  it('titles the section and holds the action', () => {
    render(
      <DangerZone description="Cannot be undone.">
        <button type="button">Delete</button>
      </DangerZone>,
    )
    expect(screen.getByRole('heading', { name: 'Danger zone' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument()
  })
})
