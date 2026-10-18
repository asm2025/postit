import { messageFor } from '@/api/errors'

/** A request or action error, translated, announced to screen readers. Renders nothing without one. */
export function ErrorText({ error }: { error: unknown }) {
  if (error == null || error === false) return null
  return (
    <p role="alert" className="text-sm text-destructive">
      {typeof error === 'string' ? error : messageFor(error)}
    </p>
  )
}
