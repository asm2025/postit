import { useTranslation } from 'react-i18next'
import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { dirOf } from '@/i18n/locales'
import { useFormat } from '@/i18n/useFormat'
import type { JobStats } from './queries'

type Bucket = JobStats['throughput'][number]

const reducedMotion = () =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false

/**
 * Stacked succeeded/failed attempts per hour, with a visually hidden table as the text alternative.
 * In a right-to-left language the time axis runs right to left and the count axis sits on the right.
 * The container keeps the page direction: recharts draws SVG with explicit coordinates, so only the
 * axes need flipping, and the HTML tooltip and legend then read in the screen language.
 */
export default function ThroughputChart({ buckets }: { buckets: Bucket[] }) {
  const { t } = useTranslation('jobs')
  const { date, dateTime, number, locale } = useFormat()
  const rtl = dirOf(locale) === 'rtl'
  const hourLabel = (iso: string) => date(iso, { hour: '2-digit', minute: '2-digit' })
  const animate = !reducedMotion()
  return (
    <div>
      <div role="img" aria-label={t('chart.label')} className="h-56 w-full">
        <ResponsiveContainer width="100%" height="100%" initialDimension={{ width: 600, height: 224 }}>
          <BarChart data={buckets} margin={{ top: 4, right: 4, bottom: 0, left: 0 }}>
            <XAxis
              dataKey="start"
              tickFormatter={hourLabel}
              reversed={rtl}
              stroke="var(--faint)"
              tick={{ fill: 'var(--faint)', fontSize: 11 }}
              minTickGap={24}
            />
            <YAxis
              allowDecimals={false}
              orientation={rtl ? 'right' : 'left'}
              tickFormatter={(v: number) => number(v)}
              width={32}
              stroke="var(--faint)"
              // Right-hand ticks are start-anchored; under an inherited rtl direction that anchors the
              // text's right edge and the digits grow over the bars, so only these ticks go ltr.
              tick={{ fill: 'var(--faint)', fontSize: 11, direction: 'ltr' }}
            />
            <Tooltip
              labelFormatter={(v) => (typeof v === 'string' ? dateTime(v) : '')}
              formatter={(v) => (typeof v === 'number' ? number(v) : v)}
              cursor={{ fill: 'var(--raised)' }}
              contentStyle={{
                background: 'var(--surface)',
                border: '1px solid var(--line)',
                borderRadius: 12,
                color: 'var(--text)',
              }}
            />
            <Bar
              dataKey="succeeded"
              name={t('chart.succeeded')}
              stackId="a"
              fill="var(--pink)"
              isAnimationActive={animate}
            />
            <Bar
              dataKey="failed"
              name={t('chart.failed')}
              stackId="a"
              fill="var(--warn-icon)"
              isAnimationActive={animate}
            />
          </BarChart>
        </ResponsiveContainer>
      </div>
      <div className="mt-2 flex gap-4 font-mono text-xs text-faint" aria-hidden="true">
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-pink" /> {t('chart.succeeded')}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-warn-icon" /> {t('chart.failed')}
        </span>
      </div>
      <table className="sr-only" aria-label={t('chart.dataLabel')}>
        <thead>
          <tr>
            <th>{t('chart.hourStarting')}</th>
            <th>{t('chart.succeeded')}</th>
            <th>{t('chart.failed')}</th>
          </tr>
        </thead>
        <tbody>
          {buckets.map((b) => (
            <tr key={b.start}>
              <td>{dateTime(b.start)}</td>
              <td>{number(b.succeeded)}</td>
              <td>{number(b.failed)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
