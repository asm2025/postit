import {
  createColumnHelper,
  createCoreRowModel,
  createSortedRowModel,
  metaHelper,
  rowSortingFeature,
  sortFn_alphanumeric,
  sortFn_basic,
  sortFn_datetime,
  sortFn_text,
  tableFeatures,
  type CellData,
  type ColumnDef,
  type RowData,
} from '@tanstack/react-table'

export interface DataTableColumnMeta {
  /** Classes on this column's header and cells: `font-mono`, `w-10`, `whitespace-normal`. */
  className?: string
  /** For values that are code, not words (a job type, a worker ID): kept left-to-right. */
  ltr?: boolean
}

/**
 * The feature set every table runs on. TanStack v9 types a table by its features, so it is declared once,
 * here; a column only gets APIs (sorting) that are registered.
 */
export const dataTableFeatures = tableFeatures({
  rowSortingFeature,
  coreRowModel: createCoreRowModel(),
  sortedRowModel: createSortedRowModel(),
  // Only the sort functions TanStack picks automatically for strings, numbers, and dates.
  sortFns: { alphanumeric: sortFn_alphanumeric, basic: sortFn_basic, datetime: sortFn_datetime, text: sortFn_text },
  columnMeta: metaHelper<DataTableColumnMeta>(),
})

export type DataTableFeatures = typeof dataTableFeatures

/** A column for `DataTable`. Sorting is off unless the column sets `enableSorting: true`. */
export type DataTableColumnDef<TData extends RowData, TValue extends CellData = CellData> = ColumnDef<
  DataTableFeatures,
  TData,
  TValue
>

/** `createColumnHelper` bound to the table's features; pages pass only their row type. */
export const columnHelper = <TData extends RowData>() => createColumnHelper<DataTableFeatures, TData>()

/** A helper-built column array, its per-column value types erased (what `helper.columns` returns). */
export type DataTableColumns<TData extends RowData> = ReturnType<ReturnType<typeof columnHelper<TData>>['columns']>
