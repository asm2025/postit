import { useTranslation } from 'react-i18next'
import { Navigate, useLocation } from 'react-router'
import { Logo } from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { useSession } from '@/auth/session'
import { LanguageSwitch } from '@/i18n/LanguageSwitch'

export function LoginPage() {
  const { t } = useTranslation(['me', 'common'])
  const { status, notice, signIn } = useSession()
  const from = (useLocation().state as { from?: string } | null)?.from ?? '/'
  if (status === 'signedIn') return <Navigate to={from} replace />
  return (
    <main className="mx-auto flex min-h-screen max-w-sm flex-col justify-center gap-4 p-6 text-center">
      <div className="mx-auto text-ink">
        <Logo className="h-[74px] w-[184px]" />
      </div>
      <h1 className="sr-only">{t('me:login.title')}</h1>
      {notice ? (
        <p role="alert" className="text-sm text-destructive">
          {notice}
        </p>
      ) : (
        <p className="text-muted-foreground">{t('me:login.prompt')}</p>
      )}
      <Button
        disabled={status === 'loading'}
        onClick={() => {
          signIn(from)
        }}
      >
        {t('common:actions.signIn')}
      </Button>
      <div className="mx-auto">
        <LanguageSwitch />
      </div>
    </main>
  )
}
