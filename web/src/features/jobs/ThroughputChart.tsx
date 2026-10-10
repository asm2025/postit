import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import type { JobStats } from './queries'

type Bucket = JobStats['throughput'][number]

const hourLabel = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
const fullLabel = (iso: string) => new Date(iso).toLocaleString()

const reducedMotion = () =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false

/** Stacked succeeded/failed attempts per hour, with a visually hidden table as the text alternative. */
export default function ThroughputChart({ buckets }: { buckets: Bucket[] }) {
  const animate = !reducedMotion()
  return (
    <div>
      <div role="img" aria-label="Jobs completed per hour, succeeded and failed" className="h-56 w-full">
        <ResponsiveContainer width="100%" height="100%" initialDimension={{ width: 600, height: 224 }}>
          <BarChart data={buckets} margin={{ top: 4, right: 4, bottom: 0, left: 0 }}>
            <XAxis
              dataKey="start"
              tickFormatter={hourLabel}
              stroke="var(--faint)"
              tick={{ fill: 'var(--faint)', fontSize: 11 }}
              minTickGap={24}
            />
            <YAxis
              allowDecimals={false}
              width={32}
              stroke="var(--faint)"
              tick={{ fill: 'var(--faint)', fontSize: 11 }}
            />
            <Tooltip
              labelFormatter={(v) => fullLabel(typeof v === 'string' ? v : '')}
              cursor={{ fill: 'var(--raised)' }}
              contentStyle={{
                background: 'var(--surface)',
                border: '1px solid var(--line)',
                borderRadius: 12,
                color: 'var(--text)',
              }}
            />
            <Bar dataKey="succeeded" name="Succeeded" stackId="a" fill="var(--pink)" isAnimationActive={animate} />
            <Bar dataKey="failed" name="Failed" stackId="a" fill="var(--warn-icon)" isAnimationActive={animate} />
          </BarChart>
        </ResponsiveContainer>
      </div>
      <div className="mt-2 flex gap-4 font-mono text-xs text-faint" aria-hidden="true">
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-pink" /> Succeeded
        </span>
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-warn-icon" /> Failed
        </span>
      </div>
      <table className="sr-only" aria-label="Throughput data">
        <thead>
          <tr>
            <th>Hour starting</th>
            <th>Succeeded</th>
            <th>Failed</th>
          </tr>
        </thead>
        <tbody>
          {buckets.map((b) => (
            <tr key={b.start}>
              <td>{fullLabel(b.start)}</td>
              <td>{b.succeeded}</td>
              <td>{b.failed}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
