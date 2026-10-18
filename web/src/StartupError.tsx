import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

/** Plain error screen shown when the runtime or auth config cannot be loaded; Retry re-runs startup. */
export function StartupError({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation()
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 p-6 text-center">
      <h1 className="font-display text-3xl font-bold ltr:tracking-[-0.02em]">{t('startup.title')}</h1>
      <p role="alert" className="text-muted-foreground">
        {message}
      </p>
      <div className="flex justify-center">
        <Button onClick={onRetry}>{t('actions.retry')}</Button>
      </div>
    </main>
  )
}
