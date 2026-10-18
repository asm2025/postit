import { flexRender, useTable, type RowData, type SortingState } from '@tanstack/react-table'
import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { dataTableFeatures, type DataTableColumns, type DataTableFeatures } from './tableColumns'
import { EmptyState } from './EmptyState'
import { TablePagination, type PaginationProps } from './TablePagination'

interface Props<TData extends RowData> {
  columns: DataTableColumns<TData>
  data: TData[] | undefined
  /** Names the table for screen readers: "Users". */
  label: string
  getRowId?: (row: TData) => string
  /** Per-row classes: dimming a row that is being deleted. */
  rowClassName?: (row: TData) => string | undefined
  /** Shown instead of the table when `data` is empty. Defaults to a generic "No results". */
  empty?: ReactNode
  /**
   * Server paging: `data` is one page and the footer drives the page. Omit for a short list shown whole
   * (no footer).
   */
  pagination?: PaginationProps
  /** For screen readers only: the data behind a chart. */
  srOnly?: boolean
}

/**
 * The only table in the app: pages describe columns and pass rows; headers, sorting, cell direction,
 * the empty state, and the paging footer are decided here. `components/ui/table` is its internals.
 */
export function DataTable<TData extends RowData>({
  columns,
  data,
  label,
  getRowId,
  rowClassName,
  empty,
  pagination,
  srOnly,
}: Props<TData>) {
  const { t } = useTranslation()
  const [sorting, setSorting] = useState<SortingState>([])
  const table = useTable<DataTableFeatures, TData>({
    features: dataTableFeatures,
    data: data ?? [],
    columns,
    getRowId: getRowId && ((row) => getRowId(row)),
    // A server-paged list only holds one page, so sorting it would mislead; columns opt in.
    defaultColumn: { enableSorting: false },
    onSortingChange: setSorting,
    state: { sorting },
  })

  if (!data) return null
  return (
    <div className={srOnly ? 'sr-only' : 'space-y-3'}>
      {data.length === 0 ? (
        (empty ?? <EmptyState>{t('table.empty')}</EmptyState>)
      ) : (
        <Table aria-label={label}>
          <TableHeader>
            {table.getHeaderGroups().map((group) => (
              <TableRow key={group.id}>
                {group.headers.map((header) => {
                  const { column } = header
                  const sorted = column.getIsSorted()
                  const content = header.isPlaceholder ? null : flexRender(column.columnDef.header, header.getContext())
                  return (
                    <TableHead
                      key={header.id}
                      className={column.columnDef.meta?.className}
                      aria-sort={sorted === 'asc' ? 'ascending' : sorted === 'desc' ? 'descending' : undefined}
                    >
                      {column.getCanSort() ? (
                        <button
                          type="button"
                          className="inline-flex min-h-11 cursor-pointer items-center gap-1"
                          onClick={column.getToggleSortingHandler()}
                        >
                          {content}
                          {sorted === 'asc' ? (
                            <ArrowUp className="size-3.5" aria-hidden="true" />
                          ) : sorted === 'desc' ? (
                            <ArrowDown className="size-3.5" aria-hidden="true" />
                          ) : (
                            <ArrowUpDown className="size-3.5 opacity-50" aria-hidden="true" />
                          )}
                        </button>
                      ) : (
                        content
                      )}
                    </TableHead>
                  )
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows.map((row) => (
              <TableRow key={row.id} className={rowClassName?.(row.original)}>
                {row.getAllCells().map((cell) => {
                  const meta = cell.column.columnDef.meta
                  return (
                    <TableCell key={cell.id} className={meta?.className}>
                      {meta?.ltr ? (
                        <bdi dir="ltr">{flexRender(cell.column.columnDef.cell, cell.getContext())}</bdi>
                      ) : (
                        flexRender(cell.column.columnDef.cell, cell.getContext())
                      )}
                    </TableCell>
                  )
                })}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {pagination && pagination.total > 0 && <TablePagination {...pagination} />}
    </div>
  )
}
