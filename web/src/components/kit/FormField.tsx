import type { ReactNode } from 'react'

/** A label stacked over its control; the wrapping label names the control. */
export function FormField({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-sm">
      {label}
      {children}
    </label>
  )
}
