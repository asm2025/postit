import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { EmptyState } from '@/components/kit/EmptyState'
import { PageHeader } from '@/components/kit/PageHeader'
import { Button } from '@/components/ui/button'
import { Loading } from '@/features/me/StatusScreens'
import { useMe } from '@/features/me/useMe'
import { useWorkspace } from '@/workspace/context'
import { useAccess } from '@/workspace/useAccess'
import { DiscoveryChecklist } from './DiscoveryChecklist'
import { isOAuthResultCode } from './oauthCodes'
import { savePending } from './pendingStore'
import { useConnectInfo } from './queries'

/**
 * Where the platform's sign-in sends the browser back to. Whoever started the connection finishes
 * it, so the page works in the user's own workspace whatever the tab was doing.
 */
export function OAuthDonePage() {
  const { t } = useTranslation(['accounts', 'common'])
  const [params] = useSearchParams()
  const result = params.get('result')
  const connectId = params.get('connect_id')
  const code = params.get('code')
  const access = useAccess()
  const { switchTo } = useWorkspace()
  const me = useMe()
  const navigate = useNavigate()
  const info = useConnectInfo(result === 'ok' ? connectId : null)

  // The one-time `connect_id` leaves the address bar and the history at once.
  useEffect(() => {
    window.history.replaceState(window.history.state, '', window.location.pathname)
  }, [])
  useEffect(() => {
    if (!access.own) switchTo(null)
  }, [access.own, switchTo])
  const userId = me.data?.id
  const view = info.data
  useEffect(() => {
    if (view && userId && connectId) {
      savePending({
        connectId,
        pendingId: view.pending_id,
        platform: view.platform,
        userId,
        expiresAt: view.expires_at,
      })
    }
  }, [view, userId, connectId])

  if (!access.own) return <Loading />
  const back = (
    <Button asChild variant="outline" className="self-start">
      <Link to="/accounts">{t('accounts:back')}</Link>
    </Button>
  )
  let body
  if (result === 'ok' && connectId) {
    if (info.isError) {
      body = (
        <>
          <EmptyState>{t('accounts:checklist.expired')}</EmptyState>
          {back}
        </>
      )
    } else if (view) {
      body = (
        <DiscoveryChecklist
          connectId={connectId}
          pendingId={view.pending_id}
          discovered={view.discovered}
          expiresAt={view.expires_at}
          onDone={() => {
            // Back to the list on cancel or expiry; a connection goes there itself.
            void navigate('/accounts')
          }}
        />
      )
    } else {
      body = <p className="text-muted-foreground">{t('common:loading')}</p>
    }
  } else {
    body = (
      <>
        <EmptyState>
          {isOAuthResultCode(code) ? t(`accounts:oauthResult.${code}`) : t('accounts:oauthResult.unknown')}
        </EmptyState>
        {back}
      </>
    )
  }
  return (
    <div className="flex flex-1 flex-col items-start gap-6">
      <PageHeader eyebrow={t('accounts:title')} title={t('accounts:connectPage.title')} />
      {body}
    </div>
  )
}
