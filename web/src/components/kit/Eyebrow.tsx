import type { ComponentProps } from 'react'
import { cn } from 'cn'

/** Mono uppercase label above a title or a group. Arabic falls back to Noto Sans Arabic, untracked (index.css). */
export function Eyebrow({ as: Tag = 'div', className, ...props }: ComponentProps<'div'> & { as?: 'div' | 'h2' }) {
  return (
    <Tag
      className={cn('font-mono text-xs font-medium tracking-[0.08em] text-muted-foreground uppercase', className)}
      {...props}
    />
  )
}
