import { ArrowRight, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Panel, PanelTitle } from '@/components/Panel'
import { Button } from '@/components/ui/button'
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

const cta =
  'inline-flex min-h-11 items-center gap-2 rounded-xl bg-pink px-[18px] font-semibold !text-on-pink no-underline hover:brightness-105'

function ApprovalQueue() {
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
  const total = pending.data?.total ?? 0
  return (
    <Panel aria-labelledby="queue-h" className="flex-[7_1_420px]">
      <PanelTitle id="queue-h">Approval queue</PanelTitle>
      <div className="flex items-baseline gap-3.5">
        <span className="font-display text-[clamp(64px,10vw,88px)] leading-[0.9] font-bold tracking-[-0.04em] tabular-nums">
          {pending.isPending ? '…' : total}
        </span>
        <span className="font-display text-[22px] font-medium">waiting</span>
      </div>
      {total === 0 && !pending.isPending && !pending.error && (
        <p className="max-w-[46ch] text-muted-foreground">
          All clear. When someone signs in for the first time, they land here as pending until you approve them — and
          you get an email.
        </p>
      )}
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
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <ul className="divide-y divide-line">
        {pending.data?.data.map((u) => (
          <li key={u.id} className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2 py-3 text-sm">
            <span className="min-w-0">
              <span className="font-semibold">{u.display_name}</span>
              <span className="text-muted-foreground">
                {' '}
                {u.email ?? ''} · {relativeTime(u.created_at)}
              </span>
            </span>
            <span className="flex gap-2">
              <Button
                className="min-h-11 px-4"
                onClick={() => {
                  run(act.approve.mutateAsync(u.id))
                }}
              >
                Approve
              </Button>
              <Button
                className="min-h-11 px-4"
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
      <div className="mt-auto flex flex-wrap gap-2.5">
        <Link to={total ? '/users?status=pending' : '/users'} className={cta}>
          Review users
          <ArrowRight className="size-4" aria-hidden="true" />
        </Link>
      </div>
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
    </Panel>
  )
}

function LegendRow({
  label,
  status,
  count,
  swatch,
  last,
}: {
  label: string
  status: 'active' | 'pending' | 'disabled'
  count: number | undefined
  swatch: string
  last?: boolean
}) {
  return (
    <div className={`flex items-center gap-2.5 py-2.5 ${last ? '' : 'border-b border-line'}`}>
      <span className={`size-2.5 rounded-[3px] ${swatch}`} aria-hidden="true" />
      <dt className="flex-1">
        <Link
          to={`/users?status=${status}`}
          aria-label={`${label} users`}
          className="inline-flex min-h-8 items-center !text-foreground no-underline hover:underline"
        >
          {label}
        </Link>
      </dt>
      <dd className="font-mono font-medium">{count ?? '–'}</dd>
    </div>
  )
}

function TeamSummary() {
  const active = useUserCount({ status: 'active' })
  const pending = useUserCount({ status: 'pending' })
  const disabled = useUserCount({ status: 'disabled' })
  const admins = useUserCount({ status: 'active', role: 'admin' })
  const a = active.data ?? 0
  const p = pending.data ?? 0
  const d = disabled.data ?? 0
  const total = a + p + d
  const ready = [active, pending, disabled].every((q) => q.data !== undefined)
  const seg = (n: number, color: string) =>
    n > 0 ? <div style={{ flex: `${String(n)} 1 0` }} className={`rounded-md ${color}`} /> : null
  return (
    <Panel aria-labelledby="team-h" className="flex-[5_1_300px]">
      <PanelTitle id="team-h">Team</PanelTitle>
      <div className="flex items-baseline gap-2.5">
        <span className="font-display text-[56px] leading-[0.9] font-bold tracking-[-0.03em]">
          {ready ? total : '…'}
        </span>
        <span className="text-muted-foreground">
          {total === 1 ? 'user' : 'users'}
          {admins.data !== undefined && ` · ${String(admins.data)} ${admins.data === 1 ? 'admin' : 'admins'}`}
        </span>
      </div>
      <div
        role="img"
        aria-label={`${String(a)} active, ${String(p)} pending, ${String(d)} disabled`}
        className="flex h-2.5 gap-[3px] overflow-hidden rounded-md bg-track"
      >
        {seg(a, 'bg-ink')}
        {seg(p, 'bg-pink')}
        {seg(d, 'bg-disabled')}
      </div>
      <dl className="flex flex-col">
        <LegendRow label="Active" status="active" count={active.data} swatch="bg-ink" />
        <LegendRow label="Pending" status="pending" count={pending.data} swatch="bg-pink" />
        <LegendRow label="Disabled" status="disabled" count={disabled.data} swatch="bg-disabled" last />
      </dl>
    </Panel>
  )
}

function RecentActivity() {
  const audit = useRecentAudit(8)
  return (
    <Panel aria-labelledby="act-h" className="flex-[7_1_420px] !gap-2">
      <div className="mb-2 flex items-center justify-between gap-3">
        <PanelTitle id="act-h">Recent activity</PanelTitle>
        <Link className="text-sm font-semibold no-underline" to="/audit">
          Full audit log →
        </Link>
      </div>
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
    </Panel>
  )
}

function AdminWarning() {
  const admins = useUserCount({ status: 'active', role: 'admin' })
  if (admins.data !== 1) return null
  return (
    <section
      aria-label="Admin coverage warning"
      role="status"
      className="flex flex-wrap items-center gap-x-5 gap-y-3.5 rounded-[14px] border border-warn-line bg-warn-bg py-3.5 pr-4 pl-[18px] text-warn-text"
    >
      <TriangleAlert className="size-5 flex-none text-warn-icon" aria-hidden="true" />
      <p className="min-w-0 flex-[1_1_360px] text-sm">
        <strong className="font-semibold">You&apos;re the only active admin.</strong> If this account is lost, nobody
        can approve new users. Promote a second person to admin.
      </p>
      <Link
        to="/users?status=active"
        className="inline-flex min-h-11 items-center rounded-[10px] border border-warn-line px-3.5 text-sm font-semibold !text-warn-text no-underline hover:border-warn-icon"
      >
        Open Users
      </Link>
    </section>
  )
}

export function AdminPanels() {
  return (
    <>
      <AdminWarning />
      <div className="flex flex-wrap gap-6">
        <ApprovalQueue />
        <TeamSummary />
      </div>
      <div className="flex flex-wrap items-start gap-6">
        <RecentActivity />
      </div>
    </>
  )
}
