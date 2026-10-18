import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useFormat } from '@/i18n/useFormat'

interface Props {
  page: number
  pageSize: number
  total: number
  onPage: (p: number) => void
}

export function Pager({ page, pageSize, total, onPage }: Props) {
  const { t } = useTranslation()
  const { number } = useFormat()
  const pages = Math.max(1, Math.ceil(total / pageSize))
  return (
    <div className="flex items-center justify-between text-sm text-muted-foreground">
      <span>{t('pager.total', { total: number(total) })}</span>
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => {
            onPage(page - 1)
          }}
        >
          {t('actions.previous')}
        </Button>
        <span>{t('pager.page', { page: number(page), pages: number(pages) })}</span>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= pages}
          onClick={() => {
            onPage(page + 1)
          }}
        >
          {t('actions.next')}
        </Button>
      </div>
    </div>
  )
}
