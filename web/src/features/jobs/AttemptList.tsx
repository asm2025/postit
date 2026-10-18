import { useTranslation } from 'react-i18next'
import { DataTable } from '@/components/kit/DataTable'
import { columnHelper } from '@/components/kit/tableColumns'
import { useFormat } from '@/i18n/useFormat'
import { formatLocale } from '@/i18n/locales'
import type { JobDetail } from './queries'
import { useStateLabel } from './labels'

type Attempt = JobDetail['attempts'][number]

export function AttemptList({ attempts }: { attempts: Attempt[] }) {
  const { t } = useTranslation('jobs')
  const { dateTime, number, locale } = useFormat()
  const stateLabel = useStateLabel()
  const seconds = new Intl.NumberFormat(formatLocale(locale), { minimumFractionDigits: 1, maximumFractionDigits: 1 })
  const duration = (a: Attempt): string => {
    if (!a.finished_at) return '—'
    const ms = new Date(a.finished_at).getTime() - new Date(a.started_at).getTime()
    return ms < 1000
      ? t('attempts.ms', { value: number(Math.max(0, ms)) })
      : t('attempts.s', { value: seconds.format(ms / 1000) })
  }

  const col = columnHelper<Attempt>()
  const columns = col.columns([
    col.accessor('attempt', {
      header: t('attempts.columns.attempt'),
      enableSorting: true,
      meta: { className: 'font-mono' },
      cell: ({ getValue }) => number(getValue()),
    }),
    col.accessor('started_at', {
      header: t('attempts.columns.started'),
      cell: ({ getValue }) => <time dateTime={getValue()}>{dateTime(getValue())}</time>,
    }),
    col.display({
      id: 'duration',
      header: t('attempts.columns.duration'),
      meta: { className: 'font-mono' },
      cell: ({ row }) => duration(row.original),
    }),
    col.accessor('worker', { header: t('attempts.columns.worker'), meta: { className: 'font-mono', ltr: true } }),
    col.accessor('outcome', {
      header: t('attempts.columns.outcome'),
      cell: ({ getValue }) => {
        const outcome = getValue()
        return outcome ? stateLabel(outcome) : '—'
      },
    }),
    col.display({
      id: 'error',
      header: t('attempts.columns.error'),
      meta: { className: 'whitespace-normal' },
      cell: ({ row: { original: a } }) => {
        const label = [a.error_kind, a.error_code].filter(Boolean).join(' / ')
        return (
          <>
            {label ? (
              <span className="font-mono text-xs" dir="ltr">
                {label}
              </span>
            ) : (
              '—'
            )}
            {a.error_message && (
              <p className="text-start text-sm text-muted-foreground" dir="ltr">
                {a.error_message}
              </p>
            )}
          </>
        )
      },
    }),
  ])

  return (
    <DataTable
      label={t('detail.attemptsTitle')}
      columns={columns}
      data={attempts}
      getRowId={(a) => String(a.attempt)}
      empty={<p className="text-sm text-faint">{t('attempts.none')}</p>}
    />
  )
}
