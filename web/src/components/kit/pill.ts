import { cn } from 'cn'

/** A pill-shaped tab or sub-navigation link; the active one is pink. */
export const pillClass = (active: boolean) =>
  cn(
    'inline-flex min-h-11 items-center rounded-lg border px-3.5 text-sm font-medium',
    active ? 'border-pink bg-pink-soft text-ink' : 'border-line bg-surface text-ink hover:bg-raised',
  )
