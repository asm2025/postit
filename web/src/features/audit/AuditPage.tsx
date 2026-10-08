import { useSearchParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { Pager } from '@/components/Pager'
import { Input } from '@/components/ui/input'
import { AuditList } from './AuditList'
import { AUDIT_KINDS, isAuditKind, KIND_LABELS } from './kinds'
import { useAudit } from './queries'

const iso = (local: string) => (local ? new Date(local).toISOString() : undefined)

export function AuditPage() {
  const [params, setParams] = useSearchParams()
  const kind = params.get('kind')
  const from = params.get('from') ?? ''
  const to = params.get('to') ?? ''
  const actor = params.get('actor') ?? ''
  const subject = params.get('subject') ?? ''
  const page = Math.max(1, Number(params.get('page') ?? 1) || 1)
  const audit = useAudit({
    kind: isAuditKind(kind) ? kind : undefined,
    from: iso(from),
    to: iso(to),
    actor: actor || undefined,
    subject: subject || undefined,
    page,
  })
  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value)
    else next.delete(key)
    if (key !== 'page') next.delete('page')
    setParams(next, { replace: true })
  }
  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Audit</h1>
      <div className="flex flex-wrap items-end gap-2 text-sm">
        <label className="flex flex-col gap-1">
          Kind
          <select
            className="rounded-md border bg-background px-2 py-1"
            value={isAuditKind(kind) ? kind : ''}
            onChange={(e) => {
              set('kind', e.target.value)
            }}
          >
            <option value="">Any kind</option>
            {AUDIT_KINDS.map((k) => (
              <option key={k} value={k}>
                {KIND_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          From
          <Input
            type="datetime-local"
            value={from}
            onChange={(e) => {
              set('from', e.target.value)
            }}
          />
        </label>
        <label className="flex flex-col gap-1">
          To
          <Input
            type="datetime-local"
            value={to}
            onChange={(e) => {
              set('to', e.target.value)
            }}
          />
        </label>
        <label className="flex flex-col gap-1">
          Actor user ID
          <Input
            value={actor}
            onChange={(e) => {
              set('actor', e.target.value.trim())
            }}
          />
        </label>
        <label className="flex flex-col gap-1">
          Subject user ID
          <Input
            value={subject}
            onChange={(e) => {
              set('subject', e.target.value.trim())
            }}
          />
        </label>
      </div>
      {audit.error && (
        <p role="alert" className="text-sm text-destructive">
          {messageFor(audit.error)}
        </p>
      )}
      {audit.data && <AuditList events={audit.data.data} />}
      {audit.data?.data.length === 0 && <p className="text-sm text-muted-foreground">No events.</p>}
      {audit.data && (
        <Pager
          page={page}
          pageSize={audit.data.page_size}
          total={audit.data.total}
          onPage={(p) => {
            set('page', String(p))
          }}
        />
      )}
    </div>
  )
}
