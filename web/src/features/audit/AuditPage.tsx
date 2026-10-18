import { useTranslation } from 'react-i18next'
import { DateRangeFilter } from '@/components/kit/DateRangeFilter'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { PageHeader } from '@/components/kit/PageHeader'
import { TablePagination } from '@/components/kit/TablePagination'
import { Input } from '@/components/ui/input'
import { useListParams } from '@/lib/listParams'
import { AuditList } from './AuditList'
import { AUDIT_KINDS, isAuditKind, useKindLabel } from './kinds'
import { useAudit } from './queries'

const iso = (local: string) => (local ? new Date(local).toISOString() : undefined)

export function AuditPage() {
  const { t } = useTranslation('audit')
  const kindLabel = useKindLabel()
  const list = useListParams()
  const kind = list.get('kind')
  const from = list.get('from')
  const to = list.get('to')
  const actor = list.get('actor')
  const subject = list.get('subject')
  const audit = useAudit({
    kind: isAuditKind(kind) ? kind : undefined,
    from: iso(from),
    to: iso(to),
    actor: actor || undefined,
    subject: subject || undefined,
    page: list.page,
    pageSize: list.pageSize,
  })
  return (
    <div className="space-y-4">
      <PageHeader title={t('title')} />
      <div className="flex flex-wrap items-end gap-2">
        <FormField label={t('filters.kind')}>
          <OptionSelect
            value={isAuditKind(kind) ? kind : ''}
            anyLabel={t('filters.anyKind')}
            options={AUDIT_KINDS.map((k) => ({ value: k, label: kindLabel(k) }))}
            onChange={(v) => {
              list.set({ kind: v })
            }}
          />
        </FormField>
        <DateRangeFilter
          from={from}
          to={to}
          onFromChange={(v) => {
            list.set({ from: v })
          }}
          onToChange={(v) => {
            list.set({ to: v })
          }}
        />
        <FormField label={t('filters.actor')}>
          <Input
            dir="ltr"
            value={actor}
            onChange={(e) => {
              list.set({ actor: e.target.value.trim() })
            }}
          />
        </FormField>
        <FormField label={t('filters.subject')}>
          <Input
            dir="ltr"
            value={subject}
            onChange={(e) => {
              list.set({ subject: e.target.value.trim() })
            }}
          />
        </FormField>
      </div>
      <ErrorText error={audit.error} />
      {audit.data && <AuditList events={audit.data.data} />}
      {audit.data?.data.length === 0 && <EmptyState>{t('none')}</EmptyState>}
      {audit.data && audit.data.total > 0 && <TablePagination {...list.paging(audit.data.total)} />}
    </div>
  )
}
