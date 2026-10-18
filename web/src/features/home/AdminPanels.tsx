import { ArrowRight, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Panel, PanelTitle } from '@/components/Panel'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { AuditList } from '@/features/audit/AuditList'
import { useRecentAudit } from '@/features/audit/queries'
import { useUserActions, useUserCount, useUsers, type User } from '@/features/users/queries'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'

function PanelError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const { t } = useTranslation('home')
  return (
    <div role="alert" className="space-y-2 text-sm text-destructive">
      <p>{messageFor(error)}</p>
      <Button size="sm" variant="outline" onClick={onRetry}>
        {t('retry')}
      </Button>
    </div>
  )
}

const cta =
  'inline-flex min-h-11 items-center gap-2 rounded-xl bg-pink px-[18px] font-semibold !text-on-pink no-underline hover:brightness-105'

function ApprovalQueue() {
  const { t } = useTranslation(['home', 'common'])
  const { relative, number } = useFormat()
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
      <PanelTitle id="queue-h">{t('home:queue.title')}</PanelTitle>
      <div className="flex items-baseline gap-3.5">
        <span className="font-display text-[clamp(64px,10vw,88px)] leading-[0.9] font-bold tracking-[-0.04em] tabular-nums">
          {pending.isPending ? '…' : number(total)}
        </span>
        <span className="font-display text-[22px] font-medium">{t('home:queue.waiting')}</span>
      </div>
      {total === 0 && !pending.isPending && !pending.error && (
        <p className="max-w-[46ch] text-muted-foreground">{t('home:queue.empty')}</p>
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
              <bdi className="font-semibold">{u.display_name}</bdi>
              <span className="text-muted-foreground">
                {' '}
                <bdi dir="ltr">{u.email ?? ''}</bdi> · {relative(u.created_at)}
              </span>
            </span>
            <span className="flex gap-2">
              <Button
                className="min-h-11 px-4"
                onClick={() => {
                  run(act.approve.mutateAsync(u.id))
                }}
              >
                {t('home:queue.approve')}
              </Button>
              <Button
                className="min-h-11 px-4"
                variant="outline"
                onClick={() => {
                  setRejecting(u)
                }}
              >
                {t('home:queue.reject')}
              </Button>
            </span>
          </li>
        ))}
      </ul>
      <div className="mt-auto flex flex-wrap gap-2.5">
        <Link to={total ? '/users?status=pending' : '/users'} className={cta}>
          {t('home:queue.review')}
          <ArrowRight className="size-4 rtl:-scale-x-100" aria-hidden="true" />
        </Link>
      </div>
      <ConfirmDialog
        open={!!rejecting}
        title={t('home:queue.rejectTitle')}
        description={t('home:queue.rejectBody', { name: isolate(rejecting?.display_name ?? '') })}
        confirmLabel={t('home:queue.reject')}
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
  linkLabel,
  status,
  count,
  swatch,
  last,
}: {
  label: string
  linkLabel: string
  status: 'active' | 'pending' | 'disabled'
  count: string | undefined
  swatch: string
  last?: boolean
}) {
  return (
    <div className={`flex items-center gap-2.5 py-2.5 ${last ? '' : 'border-b border-line'}`}>
      <span className={`size-2.5 rounded-[3px] ${swatch}`} aria-hidden="true" />
      <dt className="flex-1">
        <Link
          to={`/users?status=${status}`}
          aria-label={linkLabel}
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
  const { t } = useTranslation('home')
  const { number } = useFormat()
  const active = useUserCount({ status: 'active' })
  const pending = useUserCount({ status: 'pending' })
  const disabled = useUserCount({ status: 'disabled' })
  const admins = useUserCount({ status: 'active', role: 'admin' })
  const counts = [active, pending, disabled, admins]
  const failed = counts.find((q) => q.isError)
  const ready = [active, pending, disabled].every((q) => q.data !== undefined)
  const a = active.data ?? 0
  const p = pending.data ?? 0
  const d = disabled.data ?? 0
  const total = a + p + d
  const seg = (n: number, color: string) =>
    n > 0 ? <div style={{ flex: `${String(n)} 1 0` }} className={`rounded-md ${color}`} /> : null
  return (
    <Panel aria-labelledby="team-h" className="flex-[5_1_300px]">
      <PanelTitle id="team-h">{t('team.title')}</PanelTitle>
      {failed && (
        <PanelError
          error={failed.error}
          onRetry={() => {
            for (const q of counts) if (q.isError) void q.refetch()
          }}
        />
      )}
      {!failed && !ready && <Skeleton className="h-40 w-full" />}
      {!failed && ready && (
        <>
          <div className="flex items-baseline gap-2.5">
            <span className="font-display text-[56px] leading-[0.9] font-bold tracking-[-0.03em]">{number(total)}</span>
            <span className="text-muted-foreground">
              {t('team.users', { count: total })}
              {admins.data !== undefined && ` · ${t('team.admins', { count: admins.data, n: number(admins.data) })}`}
            </span>
          </div>
          <div
            role="img"
            aria-label={t('team.bar', { active: number(a), pending: number(p), disabled: number(d) })}
            className="flex h-2.5 gap-[3px] overflow-hidden rounded-md bg-track"
          >
            {seg(a, 'bg-ink')}
            {seg(p, 'bg-pink')}
            {seg(d, 'bg-disabled')}
          </div>
          <dl className="flex flex-col">
            <LegendRow
              label={t('team.active')}
              linkLabel={t('team.activeUsers')}
              status="active"
              count={active.data === undefined ? undefined : number(active.data)}
              swatch="bg-ink"
            />
            <LegendRow
              label={t('team.pending')}
              linkLabel={t('team.pendingUsers')}
              status="pending"
              count={pending.data === undefined ? undefined : number(pending.data)}
              swatch="bg-pink"
            />
            <LegendRow
              label={t('team.disabled')}
              linkLabel={t('team.disabledUsers')}
              status="disabled"
              count={disabled.data === undefined ? undefined : number(disabled.data)}
              swatch="bg-disabled"
              last
            />
          </dl>
        </>
      )}
    </Panel>
  )
}

function RecentActivity() {
  const { t } = useTranslation('home')
  const audit = useRecentAudit(8)
  return (
    <Panel aria-labelledby="act-h" className="flex-[7_1_420px] !gap-2">
      <div className="mb-2 flex items-center justify-between gap-3">
        <PanelTitle id="act-h">{t('activity.title')}</PanelTitle>
        <Link className="text-sm font-semibold no-underline" to="/audit">
          {t('activity.fullLog')}
          <ArrowRight className="ms-1 inline size-4 align-text-bottom rtl:-scale-x-100" aria-hidden="true" />
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
      {audit.data?.data.length === 0 && <p className="text-sm text-muted-foreground">{t('activity.empty')}</p>}
      {audit.data && <AuditList events={audit.data.data} />}
    </Panel>
  )
}

function AdminWarning() {
  const { t } = useTranslation('home')
  const admins = useUserCount({ status: 'active', role: 'admin' })
  // A failed or pending count hides the warning; the Team panel above shows the error and retry.
  if (admins.data !== 1) return null
  return (
    <section
      aria-label={t('warning.label')}
      role="status"
      className="flex flex-wrap items-center gap-x-5 gap-y-3.5 rounded-[14px] border border-warn-line bg-warn-bg py-3.5 pe-4 ps-[18px] text-warn-text"
    >
      <TriangleAlert className="size-5 flex-none text-warn-icon" aria-hidden="true" />
      <p className="min-w-0 flex-[1_1_360px] text-sm">
        <strong className="font-semibold">{t('warning.title')}</strong> {t('warning.body')}
      </p>
      <Link
        to="/users?status=active"
        className="inline-flex min-h-11 items-center rounded-[10px] border border-warn-line px-3.5 text-sm font-semibold !text-warn-text no-underline hover:border-warn-icon"
      >
        {t('warning.open')}
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
