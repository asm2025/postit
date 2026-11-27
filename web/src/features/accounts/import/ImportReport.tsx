import i18next from 'i18next'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { DataTable } from '@/components/kit/DataTable'
import { columnHelper } from '@/components/kit/tableColumns'
import { Badge } from '@/components/ui/badge'
import { useFormat } from '@/i18n/useFormat'
import { useListParams } from '@/lib/listParams'
import type { AccountImport } from './queries'
import { IMPORT_STATUSES } from './statuses'

type Row = AccountImport['rows'][number]

const translate = i18next.t.bind(i18next) as (key: string, params?: object) => string

/** A violation code in words: the shared catalog first, then the one code only an import raises. */
function violationText(code: string): string {
  if (i18next.exists(`violations:${code}`)) return translate(`violations:${code}`)
  return code === 'malformed_row' ? translate('accounts:importMalformed') : code
}

/** The per-row report of an import and how many rows ended each way. Never shows a cell value. */
export function ImportReport({ importDto }: { importDto: AccountImport }) {
  const { t } = useTranslation('accounts')
  const { number } = useFormat()
  const list = useListParams()
  const rows = importDto.rows
  const shown = rows.slice((list.page - 1) * list.pageSize, list.page * list.pageSize)

  const columns = useMemo(() => {
    const col = columnHelper<Row>()
    return col.columns([
      col.display({
        id: 'line',
        header: t('import.col.line'),
        meta: { ltr: true },
        cell: ({ row: { original: r } }) => number(r.line),
      }),
      col.display({
        id: 'status',
        header: t('import.col.status'),
        cell: ({ row: { original: r } }) => t(`importStatus.${r.status}`),
      }),
      col.display({
        id: 'problem',
        header: t('import.col.problem'),
        meta: { className: 'whitespace-normal' },
        cell: ({ row: { original: r } }) =>
          r.violations.length === 0 ? null : (
            <ul className="list-disc ps-4">
              {r.violations.map((v) => (
                <li key={v}>{violationText(v)}</li>
              ))}
            </ul>
          ),
      }),
      col.display({
        id: 'field',
        header: t('import.col.field'),
        meta: { ltr: true },
        cell: ({ row: { original: r } }) => r.field ?? '',
      }),
    ])
  }, [t, number])

  return (
    <section aria-labelledby="report-title" className="flex flex-col gap-3">
      <h2 id="report-title" className="font-display text-xl font-semibold">
        {t('import.report')}
      </h2>
      <ul className="flex flex-wrap gap-2" aria-label={t('import.counts')}>
        {IMPORT_STATUSES.filter((s) => (importDto.counts[s] ?? 0) > 0).map((s) => (
          <li key={s}>
            <Badge variant="secondary">
              {t(`importStatus.${s}`)}: {number(importDto.counts[s] ?? 0)}
            </Badge>
          </li>
        ))}
      </ul>
      <DataTable
        columns={columns}
        data={shown}
        getRowId={(r) => String(r.row)}
        label={t('import.report')}
        empty={<p className="text-sm text-muted-foreground">{t('import.noRows')}</p>}
        pagination={list.paging(rows.length)}
      />
    </section>
  )
}
