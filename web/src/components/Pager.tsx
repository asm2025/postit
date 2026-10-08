import { Button } from '@/components/ui/button'

interface Props {
  page: number
  pageSize: number
  total: number
  onPage: (p: number) => void
}

export function Pager({ page, pageSize, total, onPage }: Props) {
  const pages = Math.max(1, Math.ceil(total / pageSize))
  return (
    <div className="flex items-center justify-between text-sm text-muted-foreground">
      <span>{total} total</span>
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => {
            onPage(page - 1)
          }}
        >
          Previous
        </Button>
        <span>
          Page {page} of {pages}
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= pages}
          onClick={() => {
            onPage(page + 1)
          }}
        >
          Next
        </Button>
      </div>
    </div>
  )
}
