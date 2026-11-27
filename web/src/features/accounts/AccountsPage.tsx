import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation } from 'react-router'
import { DataTable } from '@/components/kit/DataTable'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { PlatformBadge } from '@/components/kit/PlatformBadge'
import { columnHelper } from '@/components/kit/tableColumns'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { usePlatforms } from '@/features/platforms/queries'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import { useAccess } from '@/workspace/useAccess'
import { ResumePending } from './connect/ResumePending'
import { AccountStatus } from './AccountStatus'
import { PendingConnections } from './PendingConnections'
import { useAccounts, type Account } from './queries'

/** The accounts of the active workspace, grouped by platform, and the owner's ways to add more. */
export function AccountsPage() {
  const { t } = useTranslation(['accounts', 'common'])
  const { relative } = useFormat()
  const access = useAccess()
  const list = useListParams()
  const showDisconnected = list.get('disconnected') === '1'
  const accounts = useAccounts({ includeDisconnected: showDisconnected, page: list.page, pageSize: list.pageSize })
  const platforms = usePlatforms()
  // What the connect flow reports when it sends the user back here.
  const notice = (useLocation().state as { notice?: string } | null)?.notice

  // Within a page the accounts sit together by platform, in the order the platforms are offered.
  const rows = useMemo(() => {
    const order = new Map((platforms.data ?? []).map((p, i) => [p.platform, i]))
    const rank = (a: Account) => order.get(a.platform) ?? Number.MAX_SAFE_INTEGER
    return [...(accounts.data?.data ?? [])].sort((a, b) => rank(a) - rank(b))
  }, [accounts.data, platforms.data])

  const columns = useMemo(() => {
    const col = columnHelper<Account>()
    return col.columns([
      col.display({
        id: 'name',
        header: t('accounts:col.account'),
        meta: { className: 'whitespace-normal' },
        cell: ({ row: { original: a } }) => (
          <Link to={`/accounts/${a.id}`} dir="auto" className="font-medium underline">
            {a.display_name}
          </Link>
        ),
      }),
      col.display({
        id: 'platform',
        header: t('accounts:col.platform'),
        cell: ({ row: { original: a } }) => {
          const platform = platforms.data?.find((p) => p.platform === a.platform)
          return platform ? <PlatformBadge platform={platform} size="sm" /> : <span dir="auto">{a.platform}</span>
        },
      }),
      col.display({
        id: 'status',
        header: t('accounts:col.status'),
        cell: ({ row: { original: a } }) => (
          <span className="flex flex-wrap items-center gap-1.5">
            <AccountStatus status={a.status} />
            {a.test_account && <Badge variant="outline">{t('accounts:test')}</Badge>}
          </span>
        ),
      }),
      col.display({
        id: 'checked',
        header: t('accounts:col.checked'),
        cell: ({ row: { original: a } }) => (a.last_checked_at ? relative(a.last_checked_at) : '—'),
      }),
    ])
  }, [t, platforms.data, relative])

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader title={t('accounts:title')}>
        <div className="flex flex-wrap items-center gap-2">
          {access.own && (
            <>
              <Button asChild>
                <Link to="/accounts/connect">{t('accounts:connect')}</Link>
              </Button>
              <Button asChild variant="outline">
                <Link to="/accounts/import">{t('accounts:importCsv')}</Link>
              </Button>
            </>
          )}
          <Button asChild variant="outline">
            <Link to="/accounts/groups">{t('accounts:groups')}</Link>
          </Button>
          <label className="ms-auto flex items-center gap-2 text-sm">
            <Switch
              checked={showDisconnected}
              onCheckedChange={(on) => {
                list.set({ disconnected: on ? '1' : undefined })
              }}
            />
            {t('accounts:showDisconnected')}
          </label>
        </div>
      </PageHeader>
      {notice && (
        <p role="status" className="rounded-xl border border-line bg-raised px-4 py-2 text-sm">
          {notice}
        </p>
      )}
      {access.own && <ResumePending />}
      {access.own && <PendingConnections />}
      {accounts.isError && <ErrorText error={accounts.error} />}
      <DataTable
        columns={columns}
        data={accounts.data ? rows : undefined}
        getRowId={(a) => a.id}
        label={t('accounts:title')}
        empty={<EmptyState>{t('accounts:empty')}</EmptyState>}
        pagination={accounts.data ? list.paging(accounts.data.total) : undefined}
      />
    </div>
  )
}
