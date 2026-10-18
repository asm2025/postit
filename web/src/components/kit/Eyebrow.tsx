import type { ComponentProps } from 'react'
import { cn } from 'cn'

/**
 * Small uppercase label above a title or a group. Arabic has no capitals and reads small at this size, so
 * right-to-left it is a step larger, and letter-spaced only left-to-right (spacing breaks joined letters).
 */
export function Eyebrow({ as: Tag = 'div', className, ...props }: ComponentProps<'div'> & { as?: 'div' | 'h2' }) {
  return (
    <Tag
      className={cn('text-xs font-medium text-muted-foreground uppercase ltr:tracking-[0.08em] rtl:text-sm', className)}
      {...props}
    />
  )
}
