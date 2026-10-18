import type { ReactNode } from 'react'
import { Eyebrow } from './Eyebrow'

interface Props {
  title: ReactNode
  eyebrow?: ReactNode
  /** Rendered under the title: a sub-navigation or the page's actions. */
  children?: ReactNode
}

/** The one page title: optional eyebrow, the heading, then whatever sits under it. */
export function PageHeader({ title, eyebrow, children }: Props) {
  return (
    <header className="space-y-3">
      <div className="space-y-1.5">
        {eyebrow && <Eyebrow className="text-faint">{eyebrow}</Eyebrow>}
        <h1 className="font-display text-3xl leading-tight font-bold ltr:tracking-[-0.02em]">{title}</h1>
      </div>
      {children}
    </header>
  )
}
