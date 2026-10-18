import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useSession } from '@/auth/session'
import { LanguageSwitch } from '@/i18n/LanguageSwitch'

/** `persist` saves the language to the account; only a screen the API will accept it from should ask. */
function Screen({ title, children, persist = false }: { title: string; children: ReactNode; persist?: boolean }) {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 p-6 text-center">
      <h1 className="font-display text-3xl font-bold tracking-[-0.02em]">{title}</h1>
      {children}
      <div className="mx-auto">
        <LanguageSwitch persist={persist} />
      </div>
    </main>
  )
}

export function PendingScreen({ onRefresh }: { onRefresh: () => void }) {
  const { t } = useTranslation(['me', 'common'])
  const { signOut } = useSession()
  return (
    <Screen title={t('me:pending.title')} persist>
      <p className="text-muted-foreground">{t('me:pending.body')}</p>
      <div className="flex justify-center gap-2">
        <Button onClick={onRefresh}>{t('common:actions.refresh')}</Button>
        <Button variant="outline" onClick={signOut}>
          {t('common:actions.signOut')}
        </Button>
      </div>
    </Screen>
  )
}

export function DisabledScreen() {
  const { t } = useTranslation(['me', 'common'])
  const { signOut } = useSession()
  return (
    <Screen title={t('me:disabled.title')}>
      <p className="text-muted-foreground">{t('me:disabled.body')}</p>
      <div className="flex justify-center">
        <Button variant="outline" onClick={signOut}>
          {t('common:actions.signOut')}
        </Button>
      </div>
    </Screen>
  )
}

export function ErrorScreen({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation(['me', 'common'])
  const { signOut } = useSession()
  return (
    <Screen title={t('me:error.title')}>
      <p className="text-muted-foreground">{message}</p>
      <div className="flex justify-center gap-2">
        <Button onClick={onRetry}>{t('common:actions.retry')}</Button>
        <Button variant="outline" onClick={signOut}>
          {t('common:actions.signOut')}
        </Button>
      </div>
    </Screen>
  )
}

export function Forbidden() {
  const { t } = useTranslation('me')
  return (
    <div className="p-6">
      <h1 className="font-display text-3xl font-bold">{t('forbidden.title')}</h1>
      <p className="text-muted-foreground">{t('forbidden.body')}</p>
    </div>
  )
}

export function Loading() {
  const { t } = useTranslation()
  return (
    <div className="p-6 text-muted-foreground" role="status">
      {t('loading')}
    </div>
  )
}
