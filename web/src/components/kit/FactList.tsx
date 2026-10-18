import type { ReactNode } from 'react'

/** Label/value pairs in two columns: a record's facts. Values sit at the start of their column. */
export function FactList({ facts }: { facts: readonly (readonly [ReactNode, ReactNode])[] }) {
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
      {facts.map(([k, v], i) => (
        <div key={i} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="min-w-0 break-words">{v}</dd>
        </div>
      ))}
    </dl>
  )
}
