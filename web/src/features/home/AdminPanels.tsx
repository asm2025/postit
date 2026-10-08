import { useState } from 'react'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { AuditList } from '@/features/audit/AuditList'
import { useRecentAudit } from '@/features/audit/queries'
import { useUserActions, useUserCount, useUsers, type User } from '@/features/users/queries'
import { relativeTime } from '@/lib/format'

function PanelError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div role="alert" className="space-y-2 text-sm text-destructive">
      <p>{messageFor(error)}</p>
      <Button size="sm" variant="outline" onClick={onRetry}>
        Retry
      </Button>
    </div>
  )
}

function NeedsApproval() {
  const pending = useUsers({ status: 'pending', sort: 'created_at', page: 1, pageSize: 5 }, { refetchInterval: 30_000 })
  const act = useUserActions()
  const [rejecting, setRejecting] = useState<User | null>(null)
  const [error, setError] = useState<string | null>(null)
  const run = (p: Promise<unknown>) => {
    setError(null)
    p.catch((e: unknown) => {
      setError(messageFor(e))
    })
  }
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Needs approval</CardTitle>
        {!!pending.data?.total && (
          <Link className="text-sm underline" to="/users?status=pending">
            View all ({pending.data.total})
          </Link>
        )}
      </CardHeader>
      <CardContent>
        {pending.isPending && <Skeleton className="h-16 w-full" />}
        {pending.error && (
          <PanelError
            error={pending.error}
            onRetry={() => {
              void pending.refetch()
            }}
          />
        )}
        {error && (
          <p role="alert" className="pb-2 text-sm text-destructive">
            {error}
          </p>
        )}
        {pending.data?.data.length === 0 && <p className="text-sm text-muted-foreground">No one waiting.</p>}
        <ul className="divide-y">
          {pending.data?.data.map((u) => (
            <li key={u.id} className="flex flex-wrap items-center justify-between gap-2 py-2 text-sm">
              <span>
                <span className="font-medium">{u.display_name}</span>
                <span className="text-muted-foreground">
                  {' '}
                  {u.email ?? ''} · {relativeTime(u.created_at)}
                </span>
              </span>
              <span className="flex gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    run(act.approve.mutateAsync(u.id))
                  }}
                >
                  Approve
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setRejecting(u)
                  }}
                >
                  Reject
                </Button>
              </span>
            </li>
          ))}
        </ul>
      </CardContent>
      <ConfirmDialog
        open={!!rejecting}
        title="Reject this user?"
        description={`${rejecting?.display_name ?? ''} will be removed permanently.`}
        confirmLabel="Reject"
        destructive
        busy={act.remove.isPending}
        onCancel={() => {
          setRejecting(null)
        }}
        onConfirm={() => {
          if (!rejecting) return
          run(act.remove.mutateAsync(rejecting.id))
          setRejecting(null)
        }}
      />
    </Card>
  )
}

function Counter({ label, status }: { label: string; status: 'active' | 'pending' | 'disabled' }) {
  const count = useUserCount({ status })
  return (
    <Link
      to={`/users?status=${status}`}
      className="rounded-md border p-3 hover:bg-accent/60"
      aria-label={`${label} users`}
    >
      <div className="text-2xl font-semibold">{count.isPending ? '…' : (count.data ?? '–')}</div>
      <div className="text-sm text-muted-foreground">{label}</div>
    </Link>
  )
}

function UsersAtAGlance() {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Users</CardTitle>
      </CardHeader>
      <CardContent className="grid grid-cols-3 gap-2">
        <Counter label="Active" status="active" />
        <Counter label="Pending" status="pending" />
        <Counter label="Disabled" status="disabled" />
      </CardContent>
    </Card>
  )
}

function RecentActivity() {
  const audit = useRecentAudit(8)
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Recent activity</CardTitle>
        <Link className="text-sm underline" to="/audit">
          View all
        </Link>
      </CardHeader>
      <CardContent>
        {audit.isPending && <Skeleton className="h-24 w-full" />}
        {audit.error && (
          <PanelError
            error={audit.error}
            onRetry={() => {
              void audit.refetch()
            }}
          />
        )}
        {audit.data?.data.length === 0 && <p className="text-sm text-muted-foreground">Nothing yet.</p>}
        {audit.data && <AuditList events={audit.data.data} />}
      </CardContent>
    </Card>
  )
}

function AdminWarning() {
  const admins = useUserCount({ status: 'active', role: 'admin' })
  if (admins.data !== 1) return null
  return (
    <div role="status" className="rounded-md border border-amber-500/50 bg-amber-500/10 p-3 text-sm">
      Only one active admin. If that account is lost, nobody can approve users. Promote another user in{' '}
      <Link className="underline" to="/users?status=active">
        Users
      </Link>
      .
    </div>
  )
}

export function AdminPanels() {
  return (
    <div className="space-y-4">
      <AdminWarning />
      <div className="grid gap-4 lg:grid-cols-2">
        <NeedsApproval />
        <UsersAtAGlance />
        <div className="lg:col-span-2">
          <RecentActivity />
        </div>
      </div>
    </div>
  )
}
