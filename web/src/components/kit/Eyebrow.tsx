import type { ComponentProps } from 'react'
import { cn } from 'cn'

/**
 * Mono uppercase label above a title or a group. Arabic has no capitals and reads small at this size, so
 * right-to-left it is a step larger (and untracked, in Noto Sans Arabic; see index.css).
 */
export function Eyebrow({ as: Tag = 'div', className, ...props }: ComponentProps<'div'> & { as?: 'div' | 'h2' }) {
  return (
    <Tag
      className={cn(
        'font-mono text-xs font-medium tracking-[0.08em] text-muted-foreground uppercase rtl:text-sm',
        className,
      )}
      {...props}
    />
  )
}
