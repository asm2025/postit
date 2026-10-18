import { useSearchParams } from 'react-router'

export const PAGE_SIZES = [10, 20, 50, 100] as const
export const DEFAULT_PAGE_SIZE = 20

/**
 * A list page's filters, page, and page size, kept in the URL so a reload or a shared link shows the same
 * list. Changing anything but the page goes back to page 1.
 */
export function useListParams() {
  const [params, setParams] = useSearchParams()
  const page = Math.max(1, Number(params.get('page') ?? 1) || 1)
  const size = Number(params.get('size'))
  const pageSize = (PAGE_SIZES as readonly number[]).includes(size) ? size : DEFAULT_PAGE_SIZE
  const get = (key: string) => params.get(key) ?? ''
  const set = (patch: Record<string, string | undefined>) => {
    const next = new URLSearchParams(params)
    for (const [k, v] of Object.entries(patch)) {
      if (v) next.set(k, v)
      else next.delete(k)
    }
    if (!('page' in patch)) next.delete('page')
    setParams(next, { replace: true })
  }
  return {
    get,
    set,
    page,
    pageSize,
    /** Ready to spread into `DataTable`'s `pagination` with the response's total. */
    paging: (total: number) => ({
      page,
      pageSize,
      total,
      onPageChange: (p: number) => {
        set({ page: String(p) })
      },
      onPageSizeChange: (s: number) => {
        set({ size: s === DEFAULT_PAGE_SIZE ? undefined : String(s) })
      },
    }),
  }
}
