import { MoreHorizontal } from 'lucide-react'
import type { TFunction } from 'i18next'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DataTable } from '@/components/kit/DataTable'
import { columnHelper } from '@/components/kit/tableColumns'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { PageHeader } from '@/components/kit/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { useMe } from '@/features/me/useMe'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import { useUserActions, useUsers, type User, type UserRole, type UserStatus } from './queries'

const STATUSES: UserStatus[] = ['pending', 'active', 'disabled', 'deleting']
const ROLES: UserRole[] = ['admin', 'member']

const asStatus = (v: string | null) => STATUSES.find((s) => s === v) ?? undefined
const asRole = (v: string | null) => ROLES.find((r) => r === v) ?? undefined

interface Confirm {
  title: string
  description: string
  label: string
}

interface Action {
  id: string
  label: string
  /** Asked first when set; destructive actions and changes to your own row. */
  confirm?: Confirm
  run: () => Promise<unknown>
}

/** The actions the server allows for this row (the UI never offers an invalid one). */
function actionsFor(
  u: User,
  self: boolean,
  act: ReturnType<typeof useUserActions>,
  t: TFunction<['users', 'common']>,
): Action[] {
  // Isolated so a name in the other direction cannot reorder the sentence around it.
  const name = isolate(u.display_name)
  if (u.status === 'deleting') return []
  if (u.status === 'pending') {
    return [
      { id: 'approve', label: t('users:actions.approve'), run: () => act.approve.mutateAsync(u.id) },
      {
        id: 'reject',
        label: t('users:actions.reject'),
        confirm: {
          title: t('users:confirm.rejectTitle'),
          description: t('users:confirm.rejectBody', { name }),
          label: t('users:actions.reject'),
        },
        run: () => act.remove.mutateAsync(u.id),
      },
    ]
  }
  const list: Action[] = []
  if (u.status === 'active') {
    list.push({
      id: 'disable',
      label: t('users:actions.disable'),
      confirm: self
        ? {
            title: t('users:confirm.disableSelfTitle'),
            description: t('users:confirm.disableSelfBody'),
            label: t('users:actions.disable'),
          }
        : {
            title: t('users:confirm.disableTitle'),
            description: t('users:confirm.disableBody', { name }),
            label: t('users:actions.disable'),
          },
      run: () => act.disable.mutateAsync(u.id),
    })
  } else {
    list.push({ id: 'enable', label: t('users:actions.enable'), run: () => act.enable.mutateAsync(u.id) })
  }
  if (u.role === 'member') {
    list.push({
      id: 'makeAdmin',
      label: t('users:actions.makeAdmin'),
      run: () => act.setRole.mutateAsync({ id: u.id, role: 'admin' }),
    })
  } else {
    list.push({
      id: 'makeMember',
      label: t('users:actions.makeMember'),
      confirm: self
        ? {
            title: t('users:confirm.demoteSelfTitle'),
            description: t('users:confirm.demoteSelfBody'),
            label: t('users:actions.makeMember'),
          }
        : undefined,
      run: () => act.setRole.mutateAsync({ id: u.id, role: 'member' }),
    })
  }
  if (!self) {
    list.push({
      id: 'delete',
      label: t('common:actions.delete'),
      confirm: {
        title: t('users:confirm.deleteTitle'),
        description: t('users:confirm.deleteBody', { name }),
        label: t('common:actions.delete'),
      },
      run: () => act.remove.mutateAsync(u.id),
    })
  }
  return list
}

export function UsersPage() {
  const { t } = useTranslation(['users', 'common'])
  const { relative } = useFormat()
  const list = useListParams()
  const status = asStatus(list.get('status'))
  const role = asRole(list.get('role'))
  const search = list.get('q')
  const [draft, setDraft] = useState(search)
  const [error, setError] = useState<string | null>(null)
  const [asking, setAsking] = useState<Action | null>(null)
  const me = useMe()
  const users = useUsers({ status, role, search: search || undefined, page: list.page, pageSize: list.pageSize })
  const act = useUserActions()

  useEffect(() => {
    const t = setTimeout(() => {
      if (draft !== search) list.set({ q: draft || undefined })
    }, 300)
    return () => {
      clearTimeout(t)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft])

  const run = (action: Action) => {
    setError(null)
    action.run().catch((e: unknown) => {
      setError(messageFor(e))
    })
  }
  const trigger = (action: Action) => {
    if (action.confirm) setAsking(action)
    else run(action)
  }

  const col = columnHelper<User>()
  const columns = col.columns([
    col.accessor('display_name', {
      header: t('users:columns.name'),
      cell: ({ row: { original: u } }) =>
        u.id === me.data?.id ? (
          <Link className="underline" to="/profile" dir="auto">
            {u.display_name}
          </Link>
        ) : (
          <span dir="auto">{u.display_name}</span>
        ),
    }),
    col.accessor('email', {
      header: t('users:columns.email'),
      cell: ({ getValue }) => <bdi dir="auto">{getValue() ?? '—'}</bdi>,
    }),
    col.accessor('role', { header: t('users:columns.role'), cell: ({ getValue }) => t(`common:role.${getValue()}`) }),
    col.accessor('status', {
      header: t('users:columns.status'),
      cell: ({ getValue }) => (
        <Badge variant={getValue() === 'active' ? 'default' : 'secondary'}>{t(`common:status.${getValue()}`)}</Badge>
      ),
    }),
    col.accessor('created_at', { header: t('users:columns.joined'), cell: ({ getValue }) => relative(getValue()) }),
    col.display({
      id: 'actions',
      meta: { className: 'w-10' },
      cell: ({ row: { original: u } }) => {
        const actions = actionsFor(u, u.id === me.data?.id, act, t)
        if (actions.length === 0) return null
        return (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t('users:actionsFor', { name: isolate(u.display_name) })}>
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {actions.map((a) => (
                <DropdownMenuItem
                  key={a.id}
                  onSelect={() => {
                    trigger(a)
                  }}
                >
                  {a.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )
      },
    }),
  ])

  return (
    <div className="space-y-4">
      <PageHeader title={t('users:title')} />
      <div className="flex flex-wrap gap-2">
        <Input
          type="search"
          role="searchbox"
          className="max-w-xs"
          placeholder={t('users:search')}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value)
          }}
        />
        <OptionSelect
          aria-label={t('users:filters.status')}
          value={status ?? ''}
          anyLabel={t('users:filters.anyStatus')}
          options={STATUSES.map((s) => ({ value: s, label: t(`common:status.${s}`) }))}
          onChange={(v) => {
            list.set({ status: v || undefined })
          }}
        />
        <OptionSelect
          aria-label={t('users:filters.role')}
          value={role ?? ''}
          anyLabel={t('users:filters.anyRole')}
          options={ROLES.map((r) => ({ value: r, label: t(`common:role.${r}`) }))}
          onChange={(v) => {
            list.set({ role: v || undefined })
          }}
        />
      </div>
      <ErrorText error={error} />
      <ErrorText error={users.error} />
      <DataTable
        label={t('users:title')}
        columns={columns}
        data={users.data?.data}
        getRowId={(u) => u.id}
        rowClassName={(u) => (u.status === 'deleting' ? 'opacity-60' : undefined)}
        empty={<EmptyState>{t('users:none')}</EmptyState>}
        pagination={users.data && list.paging(users.data.total)}
      />
      <ConfirmDialog
        open={asking !== null}
        title={asking?.confirm?.title ?? ''}
        description={asking?.confirm?.description ?? ''}
        confirmLabel={asking?.confirm?.label ?? ''}
        destructive
        onCancel={() => {
          setAsking(null)
        }}
        onConfirm={() => {
          if (asking) run(asking)
          setAsking(null)
        }}
      />
    </div>
  )
}
