import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DataTable } from '@/components/kit/DataTable'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { pillClass } from '@/components/kit/pill'
import { columnHelper } from '@/components/kit/tableColumns'
import { Button } from '@/components/ui/button'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import { useWorkspace } from '@/workspace/context'
import { useAccess } from '@/workspace/useAccess'
import { GrantDialog } from './GrantDialog'
import { useGranted, useLeave, useReceived, type Delegation } from './queries'

type Tab = 'granted' | 'received'

/** Who may act in whose workspace: the grants the user gave, and the ones they hold. */
export function DelegationsPage() {
  const { t } = useTranslation(['delegations', 'common'])
  const access = useAccess()
  const [params, setParams] = useSearchParams()
  // While delegated, only what the user holds can be managed; granting needs their own workspace.
  const tab: Tab = access.own && params.get('tab') !== 'received' ? 'granted' : 'received'
  const select = (next: Tab) => {
    const copy = new URLSearchParams(params)
    copy.set('tab', next)
    copy.delete('page')
    setParams(copy, { replace: true })
  }
  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader title={t('delegations:title')}>
        {access.own && (
          <div role="tablist" aria-label={t('delegations:title')} className="flex flex-wrap gap-2">
            {(['granted', 'received'] as const).map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={tab === id}
                className={`${pillClass(tab === id)} cursor-pointer`}
                onClick={() => {
                  select(id)
                }}
              >
                {t(`delegations:tab.${id}`)}
              </button>
            ))}
          </div>
        )}
      </PageHeader>
      {tab === 'granted' ? <GrantedTab /> : <ReceivedTab />}
    </div>
  )
}

function useColumns() {
  const { t } = useTranslation('delegations')
  const { date } = useFormat()
  return {
    accounts: (d: Delegation) =>
      d.all_accounts ? t('accounts.all') : t('accounts.some', { count: d.account_ids.length }),
    expiry: (d: Delegation) => (d.expires_at ? date(d.expires_at) : t('expiry.never')),
    level: (d: Delegation) => t(`level.${d.level}`),
  }
}

function GrantedTab() {
  const { t } = useTranslation(['delegations', 'common'])
  const list = useListParams()
  const granted = useGranted(list.page, list.pageSize)
  const cell = useColumns()
  const [dialog, setDialog] = useState<{ grant?: Delegation } | null>(null)

  const columns = useMemo(() => {
    const col = columnHelper<Delegation>()
    return col.columns([
      col.display({
        id: 'delegate',
        header: t('delegations:col.delegate'),
        cell: ({ row: { original: d } }) => <bdi dir="auto">{d.delegate.display_name}</bdi>,
      }),
      col.display({
        id: 'level',
        header: t('delegations:col.level'),
        cell: ({ row: { original: d } }) => cell.level(d),
      }),
      col.display({
        id: 'accounts',
        header: t('delegations:col.accounts'),
        cell: ({ row: { original: d } }) => cell.accounts(d),
      }),
      col.display({
        id: 'expiry',
        header: t('delegations:col.expiry'),
        cell: ({ row: { original: d } }) => cell.expiry(d),
      }),
      col.display({
        id: 'status',
        header: t('delegations:col.status'),
        cell: ({ row: { original: d } }) =>
          d.delegate.status === 'active' ? t('delegations:status.active') : t('delegations:status.paused'),
      }),
      col.display({
        id: 'edit',
        header: () => <span className="sr-only">{t('delegations:col.actions')}</span>,
        cell: ({ row: { original: d } }) => (
          <Button
            variant="outline"
            size="sm"
            aria-label={t('delegations:edit', { name: d.delegate.display_name })}
            onClick={() => {
              setDialog({ grant: d })
            }}
          >
            {t('common:actions.edit')}
          </Button>
        ),
      }),
    ])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, cell.level, cell.accounts, cell.expiry])

  return (
    <section className="flex flex-col gap-4">
      <div>
        <Button
          onClick={() => {
            setDialog({})
          }}
        >
          {t('delegations:grant.open')}
        </Button>
      </div>
      {granted.isError && <ErrorText error={granted.error} />}
      <DataTable
        columns={columns}
        data={granted.data?.data}
        getRowId={(d) => d.id}
        label={t('delegations:tab.granted')}
        empty={<p className="text-sm text-muted-foreground">{t('delegations:granted.empty')}</p>}
        pagination={granted.data ? list.paging(granted.data.total) : undefined}
      />
      {dialog && (
        <GrantDialog
          key={dialog.grant?.id ?? 'new'}
          open
          grant={dialog.grant}
          onClose={() => {
            setDialog(null)
          }}
        />
      )}
    </section>
  )
}

function ReceivedTab() {
  const { t } = useTranslation(['delegations', 'common'])
  const list = useListParams()
  const received = useReceived(list.page, list.pageSize)
  const leave = useLeave()
  const { actAs, switchTo } = useWorkspace()
  const cell = useColumns()
  const [leaving, setLeaving] = useState<Delegation | null>(null)

  const columns = useMemo(() => {
    const col = columnHelper<Delegation>()
    return col.columns([
      col.display({
        id: 'owner',
        header: t('delegations:col.owner'),
        cell: ({ row: { original: d } }) => <bdi dir="auto">{d.owner.display_name}</bdi>,
      }),
      col.display({
        id: 'level',
        header: t('delegations:col.level'),
        cell: ({ row: { original: d } }) => cell.level(d),
      }),
      col.display({
        id: 'accounts',
        header: t('delegations:col.accounts'),
        cell: ({ row: { original: d } }) => cell.accounts(d),
      }),
      col.display({
        id: 'expiry',
        header: t('delegations:col.expiry'),
        cell: ({ row: { original: d } }) => cell.expiry(d),
      }),
      col.display({
        id: 'actions',
        header: () => <span className="sr-only">{t('delegations:col.actions')}</span>,
        cell: ({ row: { original: d } }) => (
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={actAs === d.owner.id}
              aria-label={t('delegations:open', { name: d.owner.display_name })}
              onClick={() => {
                switchTo(d.owner.id)
              }}
            >
              {t('delegations:openWorkspace')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              aria-label={t('delegations:leave.button', { name: d.owner.display_name })}
              onClick={() => {
                setLeaving(d)
              }}
            >
              {t('delegations:leave.label')}
            </Button>
          </div>
        ),
      }),
    ])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, actAs, switchTo, cell.level, cell.accounts, cell.expiry])

  return (
    <section className="flex flex-col gap-4">
      {received.isError && <ErrorText error={received.error} />}
      <DataTable
        columns={columns}
        data={received.data?.data}
        getRowId={(d) => d.id}
        label={t('delegations:tab.received')}
        empty={<p className="text-sm text-muted-foreground">{t('delegations:received.empty')}</p>}
        pagination={received.data ? list.paging(received.data.total) : undefined}
      />
      <ErrorText error={leave.error ? messageFor(leave.error) : null} />
      <ConfirmDialog
        open={leaving !== null}
        title={t('delegations:leave.title')}
        description={leaving ? t('delegations:leave.confirm', { name: isolate(leaving.owner.display_name) }) : ''}
        confirmLabel={t('delegations:leave.label')}
        destructive
        busy={leave.isPending}
        onCancel={() => {
          setLeaving(null)
        }}
        onConfirm={() => {
          if (!leaving) return
          const target = leaving
          leave.mutate(target.id, {
            onSuccess: () => {
              setLeaving(null)
              // Leaving the workspace the tab is in would leave the tab acting for nobody.
              if (actAs === target.owner.id) switchTo(null)
            },
          })
        }}
      />
    </section>
  )
}
