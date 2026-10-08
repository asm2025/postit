import { MoreHorizontal } from 'lucide-react'
import { useEffect, useState } from 'react'
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
import { relativeTime } from '@/lib/format'
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
  label: string
  /** Asked first when set; destructive actions and changes to your own row. */
  confirm?: Confirm
  run: () => Promise<unknown>
}

const LOSE_ADMIN = 'You will lose admin access as soon as this is saved.'

/** The actions the server allows for this row (the UI never offers an invalid one). */
function actionsFor(u: User, self: boolean, act: ReturnType<typeof useUserActions>): Action[] {
  const name = u.display_name
  if (u.status === 'deleting') return []
  if (u.status === 'pending') {
    return [
      { label: 'Approve', run: () => act.approve.mutateAsync(u.id) },
      {
        label: 'Reject',
        confirm: { title: 'Reject this user?', description: `${name} will be removed permanently.`, label: 'Reject' },
        run: () => act.remove.mutateAsync(u.id),
      },
    ]
  }
  const list: Action[] = []
  if (u.status === 'active') {
    list.push({
      label: 'Disable',
      confirm: self
        ? {
            title: 'Disable your own account?',
            description: `You will be locked out of postit. ${LOSE_ADMIN}`,
            label: 'Disable',
          }
        : { title: 'Disable this user?', description: `${name} cannot use postit until re-enabled.`, label: 'Disable' },
      run: () => act.disable.mutateAsync(u.id),
    })
  } else {
    list.push({ label: 'Re-enable', run: () => act.enable.mutateAsync(u.id) })
  }
  if (u.role === 'member') {
    list.push({ label: 'Make admin', run: () => act.setRole.mutateAsync({ id: u.id, role: 'admin' }) })
  } else {
    list.push({
      label: 'Make member',
      confirm: self
        ? { title: 'Remove your own admin role?', description: LOSE_ADMIN, label: 'Make member' }
        : undefined,
      run: () => act.setRole.mutateAsync({ id: u.id, role: 'member' }),
    })
  }
  if (!self) {
    list.push({
      label: 'Delete',
      confirm: { title: 'Delete this user?', description: `${name} will be removed permanently.`, label: 'Delete' },
      run: () => act.remove.mutateAsync(u.id),
    })
  }
  return list
}

export function UsersPage() {
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
      <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">Users</h1>
      <div className="flex flex-wrap gap-2">
        <Input
          type="search"
          role="searchbox"
          className="max-w-xs"
          placeholder="Search name or email"
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value)
          }}
        />
        <select
          aria-label="Status"
          className="min-h-11 rounded-lg border border-line bg-surface px-2.5 text-sm"
          value={status ?? ''}
          onChange={(e) => {
            set({ status: e.target.value || undefined })
          }}
        >
          <option value="">Any status</option>
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <select
          aria-label="Role"
          className="min-h-11 rounded-lg border border-line bg-surface px-2.5 text-sm"
          value={role ?? ''}
          onChange={(e) => {
            set({ role: e.target.value || undefined })
          }}
        >
          <option value="">Any role</option>
          {ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
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
            <TableHead>Name</TableHead>
            <TableHead>Email</TableHead>
            <TableHead>Role</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Joined</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {users.data?.data.map((u) => {
            const self = u.id === me.data?.id
            const actions = actionsFor(u, self, act)
            return (
              <TableRow key={u.id} className={u.status === 'deleting' ? 'opacity-60' : undefined}>
                <TableCell>
                  {self ? (
                    <Link className="underline" to="/profile">
                      {u.display_name}
                    </Link>
                  ) : (
                    u.display_name
                  )}
                </TableCell>
                <TableCell>{u.email ?? '—'}</TableCell>
                <TableCell>{u.role}</TableCell>
                <TableCell>
                  <Badge variant={u.status === 'active' ? 'default' : 'secondary'}>{u.status}</Badge>
                </TableCell>
                <TableCell>{relativeTime(u.created_at)}</TableCell>
                <TableCell>
                  {actions.length > 0 && (
                    <DropdownMenu>
                      <DropdownMenuTrigger
                        render={<Button variant="ghost" size="icon" aria-label={`Actions for ${u.display_name}`} />}
                      >
                        <MoreHorizontal />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        {actions.map((a) => (
                          <DropdownMenuItem
                            key={a.label}
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
      {users.data?.data.length === 0 && <p className="text-sm text-muted-foreground">No users match.</p>}
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
