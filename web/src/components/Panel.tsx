import type { ComponentProps } from 'react'
import { cn } from 'cn'

/** Rounded surface section used across the dashboard (mockup: radius 20, padding 28). */
export function Panel({ className, ...props }: ComponentProps<'section'>) {
  return (
    <section
      className={cn(
        'flex min-w-0 flex-col gap-[18px] rounded-[20px] border border-line bg-surface p-5 sm:p-7',
        className,
      )}
      {...props}
    />
  )
}

/** Mono uppercase eyebrow heading. */
export function PanelTitle({ className, ...props }: ComponentProps<'h2'>) {
  return (
    <h2
      className={cn('font-mono text-xs font-medium tracking-[0.08em] text-muted-foreground uppercase', className)}
      {...props}
    />
  )
}
