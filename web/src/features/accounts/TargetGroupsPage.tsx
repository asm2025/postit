import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DataTable } from '@/components/kit/DataTable'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { columnHelper } from '@/components/kit/tableColumns'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { isolate } from '@/i18n/bidi'
import { useListParams } from '@/lib/listParams'
import { useAccess } from '@/workspace/useAccess'
import { TargetGroupDialog } from './TargetGroupDialog'
import { useAccounts, useDeleteTargetGroup, useTargetGroups, type TargetGroup } from './queries'

/** Named sets of accounts. Anyone with access can read them; editing needs the `edit` level. */
export function TargetGroupsPage() {
  const { t } = useTranslation(['accounts', 'common'])
  const access = useAccess()
  const canEdit = access.atLeast('edit')
  const list = useListParams()
  const groups = useTargetGroups(list.page, list.pageSize)
  const accounts = useAccounts({ page: 1, pageSize: 100 })
  const remove = useDeleteTargetGroup()
  const [dialog, setDialog] = useState<{ group?: TargetGroup } | null>(null)
  const [deleting, setDeleting] = useState<TargetGroup | null>(null)

  const names = useMemo(() => new Map((accounts.data?.data ?? []).map((a) => [a.id, a.display_name])), [accounts.data])

  const columns = useMemo(() => {
    const col = columnHelper<TargetGroup>()
    return col.columns([
      col.display({
        id: 'name',
        header: t('accounts:groupsPage.col.name'),
        cell: ({ row: { original: g } }) => (
          <bdi dir="auto" className="font-medium">
            {g.name}
          </bdi>
        ),
      }),
      col.display({
        id: 'accounts',
        header: t('accounts:groupsPage.col.accounts'),
        meta: { className: 'whitespace-normal' },
        cell: ({ row: { original: g } }) =>
          g.account_ids.length === 0 ? (
            <span className="text-muted-foreground">{t('accounts:groupsPage.noAccounts')}</span>
          ) : (
            <span className="flex flex-wrap gap-1">
              {g.account_ids.map((id) => (
                <Badge key={id} variant="secondary">
                  <bdi dir="auto">{names.get(id) ?? '…'}</bdi>
                </Badge>
              ))}
            </span>
          ),
      }),
      ...(canEdit
        ? [
            col.display({
              id: 'actions',
              header: () => <span className="sr-only">{t('accounts:groupsPage.col.actions')}</span>,
              cell: ({ row: { original: g } }) => (
                <div className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label={t('accounts:groupsPage.edit', { name: g.name })}
                    onClick={() => {
                      setDialog({ group: g })
                    }}
                  >
                    {t('common:actions.edit')}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label={t('accounts:groupsPage.delete', { name: g.name })}
                    onClick={() => {
                      setDeleting(g)
                    }}
                  >
                    {t('common:actions.delete')}
                  </Button>
                </div>
              ),
            }),
          ]
        : []),
    ])
  }, [t, names, canEdit])

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('accounts:title')} title={t('accounts:groupsPage.title')}>
        <p className="text-sm text-muted-foreground">{t('accounts:groupsPage.intro')}</p>
        {canEdit && (
          <div>
            <Button
              onClick={() => {
                setDialog({})
              }}
            >
              {t('accounts:groupsPage.create')}
            </Button>
          </div>
        )}
      </PageHeader>
      {groups.isError && <ErrorText error={groups.error} />}
      <DataTable
        columns={columns}
        data={groups.data?.data}
        getRowId={(g) => g.id}
        label={t('accounts:groupsPage.title')}
        empty={<p className="text-sm text-muted-foreground">{t('accounts:groupsPage.empty')}</p>}
        pagination={groups.data ? list.paging(groups.data.total) : undefined}
      />
      {dialog && (
        <TargetGroupDialog
          key={dialog.group?.id ?? 'new'}
          group={dialog.group}
          onClose={() => {
            setDialog(null)
          }}
        />
      )}
      <ErrorText error={remove.error ? messageFor(remove.error) : null} />
      <ConfirmDialog
        open={deleting !== null}
        title={t('accounts:groupsPage.deleteConfirm.title')}
        description={deleting ? t('accounts:groupsPage.deleteConfirm.body', { name: isolate(deleting.name) }) : ''}
        confirmLabel={t('accounts:groupsPage.deleteConfirm.button')}
        destructive
        busy={remove.isPending}
        onCancel={() => {
          setDeleting(null)
        }}
        onConfirm={() => {
          if (!deleting) return
          remove.mutate(deleting.id, {
            onSuccess: () => {
              setDeleting(null)
            },
          })
        }}
      />
    </div>
  )
}
