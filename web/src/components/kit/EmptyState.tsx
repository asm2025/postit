import type { ReactNode } from 'react'

/** What a list says when it has nothing to show. */
export function EmptyState({ children }: { children: ReactNode }) {
  return <p className="text-sm text-muted-foreground">{children}</p>
}
