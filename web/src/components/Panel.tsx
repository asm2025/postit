import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Eyebrow } from '@/components/kit/Eyebrow'

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

/** A panel's heading: the eyebrow style, as an `h2`. */
export function PanelTitle(props: ComponentProps<'h2'>) {
  return <Eyebrow as="h2" {...props} />
}
