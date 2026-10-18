import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useFormat } from '@/i18n/useFormat'
import { PAGE_SIZES } from '@/lib/listParams'

export interface PaginationProps {
  /** 1-based. */
  page: number
  pageSize: number
  total: number
  onPageChange: (page: number) => void
  /** Offers the page-size menu when set. */
  onPageSizeChange?: (size: number) => void
}

/**
 * The footer under every paged list: page size, "first–last of total", previous, a page box, next. One
 * cluster at the inline end that never wraps; the chevrons point the reading direction.
 */
export function TablePagination({ page, pageSize, total, onPageChange, onPageSizeChange }: PaginationProps) {
  const { t } = useTranslation()
  const { number } = useFormat()
  const sizeLabel = useId()
  const pages = Math.max(1, Math.ceil(total / pageSize))
  const first = total === 0 ? 0 : (page - 1) * pageSize + 1
  const last = Math.min(page * pageSize, total)
  // What is typed in the page box; it belongs to one page, so a page change elsewhere resets it.
  const [typed, setTyped] = useState({ page, text: String(page) })
  const draft = typed.page === page ? typed.text : String(page)
  const setDraft = (text: string) => {
    setTyped({ page, text })
  }

  const commit = () => {
    const n = Number(draft.trim())
    if (!Number.isInteger(n) || n < 1) {
      setDraft(String(page))
      return
    }
    const clamped = Math.min(n, pages)
    setDraft(String(clamped))
    if (clamped !== page) onPageChange(clamped)
  }

  return (
    <div className="flex items-center justify-end gap-3 overflow-x-auto text-sm text-muted-foreground">
      {onPageSizeChange && (
        <div className="flex shrink-0 items-center gap-2 whitespace-nowrap">
          <span id={sizeLabel}>{t('pager.pageSize')}</span>
          <Select
            value={String(pageSize)}
            onValueChange={(v) => {
              onPageSizeChange(Number(v))
            }}
          >
            <SelectTrigger aria-labelledby={sizeLabel} className="w-20">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PAGE_SIZES.map((s) => (
                <SelectItem key={s} value={String(s)}>
                  {number(s)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}
      <span className="shrink-0 whitespace-nowrap">
        {t('pager.range', { first: number(first), last: number(last), total: number(total) })}
      </span>
      <div className="flex shrink-0 items-center gap-2">
        <Button
          variant="outline"
          size="icon"
          aria-label={t('pager.previous')}
          disabled={page <= 1}
          onClick={() => {
            onPageChange(page - 1)
          }}
        >
          <ChevronLeft className="rtl:-scale-x-100" aria-hidden="true" />
        </Button>
        <Input
          type="text"
          inputMode="numeric"
          aria-label={t('pager.pageNumber', { pages: number(pages) })}
          className="w-14 text-center"
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value)
          }}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              commit()
            }
          }}
        />
        <Button
          variant="outline"
          size="icon"
          aria-label={t('pager.next')}
          disabled={page >= pages}
          onClick={() => {
            onPageChange(page + 1)
          }}
        >
          <ChevronRight className="rtl:-scale-x-100" aria-hidden="true" />
        </Button>
      </div>
    </div>
  )
}
