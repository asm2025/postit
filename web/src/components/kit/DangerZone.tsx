import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

interface Props {
  /** Say what is destroyed and that it cannot be undone. */
  description: ReactNode
  /** The destructive action(s), aligned to the inline end. */
  children: ReactNode
  title?: ReactNode
}

/** The bordered section that holds irreversible actions, set apart from everything else on a page. */
export function DangerZone({ description, children, title }: Props) {
  const { t } = useTranslation()
  return (
    <section className="space-y-3 rounded-xl border border-destructive/40 p-4">
      <div>
        <h2 className="text-base font-semibold text-destructive">{title ?? t('dangerZone.title')}</h2>
        <p className="mt-1 text-sm text-muted-foreground">{description}</p>
      </div>
      <div className="flex justify-end gap-3">{children}</div>
    </section>
  )
}
