import { Bell } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { ErrorText } from '@/components/kit/ErrorText'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useFormat } from '@/i18n/useFormat'
import { subjectOf } from './links'
import { useMarkRead, useNotifications, useUnreadCount } from './queries'
import { useKindLabel } from './useKindLabel'
import { useOpenNotification } from './useOpenNotification'

const NEWEST = 10

/** The bell in the header: the unread count, and the newest notifications in a popover. */
export function NotificationBell() {
  const { t } = useTranslation('notifications')
  const { number, relative } = useFormat()
  const [open, setOpen] = useState(false)
  const unread = useUnreadCount()
  const newest = useNotifications(1, NEWEST)
  const markRead = useMarkRead()
  const openNotification = useOpenNotification()
  const kindLabel = useKindLabel()
  const count = unread.data ?? 0
  const badge = count > 99 ? t('bell.many') : number(count)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={count > 0 ? t('bell.labelUnread', { count }) : t('bell.label')}
          className="relative flex size-11 cursor-pointer items-center justify-center rounded-[10px] border border-line bg-surface"
        >
          <Bell className="size-5" aria-hidden="true" />
          {count > 0 && (
            <span
              aria-hidden="true"
              className="absolute -top-1 -end-1 min-w-5 rounded-full bg-pink px-1 text-center text-[11px] leading-5 font-bold text-on-pink"
            >
              {badge}
            </span>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 gap-2">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-sm font-semibold">{t('title')}</h2>
          <Button
            variant="ghost"
            size="xs"
            disabled={count === 0 || markRead.isPending}
            onClick={() => {
              markRead.mutate({ all: true })
            }}
          >
            {t('bell.markAll')}
          </Button>
        </div>
        {newest.isError && <ErrorText error={newest.error} />}
        {newest.data?.data.length === 0 && <p className="text-sm text-muted-foreground">{t('bell.empty')}</p>}
        <ul className="flex max-h-80 flex-col gap-1 overflow-y-auto">
          {newest.data?.data.map((n) => (
            <li key={n.id}>
              <button
                type="button"
                onClick={() => {
                  setOpen(false)
                  openNotification(n)
                }}
                className="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-lg border border-transparent bg-transparent p-2 text-start hover:bg-raised"
              >
                <span className={n.read_at ? 'text-sm' : 'text-sm font-semibold'}>
                  {n.read_at ? null : <span className="sr-only">{t('unread')} </span>}
                  {kindLabel(n.kind)}
                </span>
                {subjectOf(n) && (
                  <span dir="auto" className="text-xs text-muted-foreground">
                    {subjectOf(n)}
                  </span>
                )}
                <span className="text-xs text-faint">{relative(n.created_at)}</span>
              </button>
            </li>
          ))}
        </ul>
        <Link
          to="/notifications"
          onClick={() => {
            setOpen(false)
          }}
          className="text-sm underline"
        >
          {t('bell.seeAll')}
        </Link>
      </PopoverContent>
    </Popover>
  )
}
