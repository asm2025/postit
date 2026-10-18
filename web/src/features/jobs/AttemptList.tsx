import { useTranslation } from 'react-i18next'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
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
  if (attempts.length === 0) return <p className="text-sm text-faint">{t('attempts.none')}</p>
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>{t('attempts.columns.attempt')}</TableHead>
          <TableHead>{t('attempts.columns.started')}</TableHead>
          <TableHead>{t('attempts.columns.duration')}</TableHead>
          <TableHead>{t('attempts.columns.worker')}</TableHead>
          <TableHead>{t('attempts.columns.outcome')}</TableHead>
          <TableHead>{t('attempts.columns.error')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {attempts.map((a) => {
          const label = [a.error_kind, a.error_code].filter(Boolean).join(' / ')
          return (
            <TableRow key={a.attempt}>
              <TableCell className="font-mono">{number(a.attempt)}</TableCell>
              <TableCell>
                <time dateTime={a.started_at}>{dateTime(a.started_at)}</time>
              </TableCell>
              <TableCell className="font-mono">{duration(a)}</TableCell>
              <TableCell className="font-mono" dir="ltr">
                {a.worker}
              </TableCell>
              <TableCell>{a.outcome ? stateLabel(a.outcome) : '—'}</TableCell>
              <TableCell className="whitespace-normal">
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
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}
