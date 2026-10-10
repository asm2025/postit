import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { JobDetail } from './queries'

type Attempt = JobDetail['attempts'][number]

function duration(a: Attempt): string {
  if (!a.finished_at) return '—'
  const ms = new Date(a.finished_at).getTime() - new Date(a.started_at).getTime()
  return ms < 1000 ? `${String(Math.max(0, ms))} ms` : `${(ms / 1000).toFixed(1)} s`
}

export function AttemptList({ attempts }: { attempts: Attempt[] }) {
  if (attempts.length === 0) return <p className="text-sm text-faint">No attempts yet.</p>
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Attempt</TableHead>
          <TableHead>Started</TableHead>
          <TableHead>Duration</TableHead>
          <TableHead>Worker</TableHead>
          <TableHead>Outcome</TableHead>
          <TableHead>Error</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {attempts.map((a) => {
          const label = [a.error_kind, a.error_code].filter(Boolean).join(' / ')
          return (
            <TableRow key={a.attempt}>
              <TableCell className="font-mono">{a.attempt}</TableCell>
              <TableCell>
                <time dateTime={a.started_at}>{new Date(a.started_at).toLocaleString()}</time>
              </TableCell>
              <TableCell className="font-mono">{duration(a)}</TableCell>
              <TableCell className="font-mono">{a.worker}</TableCell>
              <TableCell>{a.outcome ?? '—'}</TableCell>
              <TableCell className="whitespace-normal">
                {label ? <span className="font-mono text-xs">{label}</span> : '—'}
                {a.error_message && <p className="text-sm text-muted-foreground">{a.error_message}</p>}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}
