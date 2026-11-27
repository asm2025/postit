import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import type { StartupReason } from '@/config/runtime'

/** Plain error screen shown when startup fails; Retry re-runs it. An unknown failure still gets a generic line. */
export function StartupError({ reason, onRetry }: { reason?: StartupReason; onRetry: () => void }) {
  const { t } = useTranslation()
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 p-6 text-center">
      <h1 className="font-display text-3xl font-bold ltr:tracking-[-0.02em]">{t('startup.title')}</h1>
      <p role="alert" className="text-muted-foreground">
        {t(`startup.reason.${reason ?? 'unknown'}`)}
      </p>
      <div className="flex justify-center">
        <Button onClick={onRetry}>{t('actions.retry')}</Button>
      </div>
    </main>
  )
}
