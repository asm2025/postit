import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { DataTable } from '@/components/kit/DataTable'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { columnHelper } from '@/components/kit/tableColumns'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useMe } from '@/features/me/useMe'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import { linkFor, subjectOf } from './links'
import { useMarkRead, useNotifications, useUnreadCount, type NotificationDto } from './queries'
import { useKindLabel } from './useKindLabel'
import { useOpenNotification } from './useOpenNotification'

/** Every notification of the signed-in user, newest first, with its read state and a way to open it. */
export function NotificationsPage() {
  const { t } = useTranslation(['notifications', 'common'])
  const { dateTime } = useFormat()
  const list = useListParams()
  const notifications = useNotifications(list.page, list.pageSize)
  const unread = useUnreadCount()
  const markRead = useMarkRead()
  const open = useOpenNotification()
  const kindLabel = useKindLabel()
  const me = useMe()
  const myId = me.data?.id ?? ''

  const columns = useMemo(() => {
    const col = columnHelper<NotificationDto>()
    return col.columns([
      col.display({
        id: 'what',
        header: t('notifications:page.what'),
        meta: { className: 'whitespace-normal' },
        cell: ({ row: { original: n } }) => (
          <div className="flex flex-col">
            <span className={n.read_at ? undefined : 'font-semibold'}>{kindLabel(n.kind)}</span>
            {subjectOf(n) && (
              <span dir="auto" className="text-xs text-muted-foreground">
                {subjectOf(n)}
              </span>
            )}
          </div>
        ),
      }),
      col.display({
        id: 'severity',
        header: t('notifications:page.severity'),
        cell: ({ row: { original: n } }) => (
          <Badge variant={n.severity === 'critical' ? 'destructive' : 'secondary'}>
            {t(`notifications:severity.${n.severity}`)}
          </Badge>
        ),
      }),
      col.display({
        id: 'state',
        header: t('notifications:page.state'),
        cell: ({ row: { original: n } }) => (n.read_at ? t('notifications:read') : t('notifications:unread')),
      }),
      col.display({
        id: 'when',
        header: t('notifications:page.when'),
        cell: ({ row: { original: n } }) => dateTime(n.created_at),
      }),
      col.display({
        id: 'open',
        header: () => <span className="sr-only">{t('notifications:page.actions')}</span>,
        cell: ({ row: { original: n } }) =>
          linkFor(n, myId) ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                open(n)
              }}
            >
              {t('notifications:page.open')}
            </Button>
          ) : null,
      }),
    ])
  }, [t, dateTime, kindLabel, open, myId])

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader title={t('notifications:title')}>
        <Button
          variant="outline"
          disabled={!unread.data || markRead.isPending}
          onClick={() => {
            markRead.mutate({ all: true })
          }}
        >
          {t('notifications:bell.markAll')}
        </Button>
      </PageHeader>
      {notifications.isError && <ErrorText error={notifications.error} />}
      <DataTable
        columns={columns}
        data={notifications.data?.data}
        getRowId={(n) => n.id}
        label={t('notifications:title')}
        empty={<p className="text-sm text-muted-foreground">{t('notifications:page.empty')}</p>}
        pagination={notifications.data ? list.paging(notifications.data.total) : undefined}
      />
    </div>
  )
}
