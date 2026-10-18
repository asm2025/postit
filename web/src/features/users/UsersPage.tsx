import { MoreHorizontal } from 'lucide-react'
import type { TFunction } from 'i18next'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useSearchParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Pager } from '@/components/Pager'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useMe } from '@/features/me/useMe'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
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
  const [params, setParams] = useSearchParams()
  const status = asStatus(params.get('status'))
  const role = asRole(params.get('role'))
  const search = params.get('q') ?? ''
  const page = Math.max(1, Number(params.get('page') ?? 1) || 1)
  const [draft, setDraft] = useState(search)
  const [error, setError] = useState<string | null>(null)
  const [asking, setAsking] = useState<Action | null>(null)
  const me = useMe()
  const users = useUsers({ status, role, search: search || undefined, page })
  const act = useUserActions()

  const set = (patch: Record<string, string | undefined>) => {
    const next = new URLSearchParams(params)
    for (const [k, v] of Object.entries(patch)) {
      if (v) next.set(k, v)
      else next.delete(k)
    }
    if (!('page' in patch)) next.delete('page')
    setParams(next, { replace: true })
  }

  useEffect(() => {
    const t = setTimeout(() => {
      if (draft !== search) set({ q: draft || undefined })
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

  return (
    <div className="space-y-4">
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">{t('users:title')}</h1>
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
        <select
          aria-label={t('users:filters.status')}
          className="min-h-11 rounded-lg border border-line bg-surface px-2.5 text-sm"
          value={status ?? ''}
          onChange={(e) => {
            set({ status: e.target.value || undefined })
          }}
        >
          <option value="">{t('users:filters.anyStatus')}</option>
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {t(`common:status.${s}`)}
            </option>
          ))}
        </select>
        <select
          aria-label={t('users:filters.role')}
          className="min-h-11 rounded-lg border border-line bg-surface px-2.5 text-sm"
          value={role ?? ''}
          onChange={(e) => {
            set({ role: e.target.value || undefined })
          }}
        >
          <option value="">{t('users:filters.anyRole')}</option>
          {ROLES.map((r) => (
            <option key={r} value={r}>
              {t(`common:role.${r}`)}
            </option>
          ))}
        </select>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {users.error && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(users.error)}
        </p>
      )}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('users:columns.name')}</TableHead>
            <TableHead>{t('users:columns.email')}</TableHead>
            <TableHead>{t('users:columns.role')}</TableHead>
            <TableHead>{t('users:columns.status')}</TableHead>
            <TableHead>{t('users:columns.joined')}</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {users.data?.data.map((u) => {
            const self = u.id === me.data?.id
            const actions = actionsFor(u, self, act, t)
            return (
              <TableRow key={u.id} className={u.status === 'deleting' ? 'opacity-60' : undefined}>
                <TableCell>
                  {self ? (
                    <Link className="underline" to="/profile" dir="auto">
                      {u.display_name}
                    </Link>
                  ) : (
                    <span dir="auto">{u.display_name}</span>
                  )}
                </TableCell>
                <TableCell dir="auto">{u.email ?? '—'}</TableCell>
                <TableCell>{t(`common:role.${u.role}`)}</TableCell>
                <TableCell>
                  <Badge variant={u.status === 'active' ? 'default' : 'secondary'}>
                    {t(`common:status.${u.status}`)}
                  </Badge>
                </TableCell>
                <TableCell>{relative(u.created_at)}</TableCell>
                <TableCell>
                  {actions.length > 0 && (
                    <DropdownMenu>
                      <DropdownMenuTrigger
                        render={
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label={t('users:actionsFor', { name: isolate(u.display_name) })}
                          />
                        }
                      >
                        <MoreHorizontal />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        {actions.map((a) => (
                          <DropdownMenuItem
                            key={a.id}
                            onClick={() => {
                              trigger(a)
                            }}
                          >
                            {a.label}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
      {users.data?.data.length === 0 && <p className="text-sm text-muted-foreground">{t('users:none')}</p>}
      {users.data && (
        <Pager
          page={page}
          pageSize={users.data.page_size}
          total={users.data.total}
          onPage={(p) => {
            set({ page: String(p) })
          }}
        />
      )}
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
