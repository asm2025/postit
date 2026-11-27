import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useParams } from 'react-router'
import { messageFor } from '@/api/errors'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { PlatformBadge } from '@/components/kit/PlatformBadge'
import { Button } from '@/components/ui/button'
import { useMe } from '@/features/me/useMe'
import { usePlatforms, type Platform } from '@/features/platforms/queries'
import { CredentialsStep } from './CredentialsStep'
import { DiscoveryChecklist } from './DiscoveryChecklist'
import { savePending } from './pendingStore'
import { useStartOAuth, type CredentialCheck } from './queries'

/**
 * Where the platform sends the browser back to. The server matches `{app.public_url}/oauth-done`
 * exactly, as a string, so this works only where `app.public_url` is the web app's origin with
 * no path, which every `backend/config/*.toml` has.
 */
const returnTo = () => `${window.location.origin}/oauth-done`

/** Sends the browser to the platform's sign-in. */
function useOAuthRedirect() {
  const start = useStartOAuth()
  return {
    error: start.error,
    pending: start.isPending,
    begin: (platform: string) => {
      start.mutate(
        { platform, returnTo: returnTo() },
        {
          onSuccess: (started) => {
            window.location.assign(started.authorize_url)
          },
        },
      )
    },
  }
}

/** Choose a platform, then connect it by credentials or through the platform's sign-in. */
export function ConnectPage() {
  const { t } = useTranslation(['accounts', 'common'])
  const { platform: id } = useParams()
  const platforms = usePlatforms()
  if (platforms.isError) return <ErrorText error={platforms.error} />
  if (!platforms.data) return <p className="text-muted-foreground">{t('common:loading')}</p>
  const chosen = id === undefined ? undefined : platforms.data.find((p) => p.platform === id)
  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('accounts:title')} title={t('accounts:connectPage.title')}>
        <Button asChild variant="outline" size="sm">
          <Link to="/accounts">{t('accounts:back')}</Link>
        </Button>
      </PageHeader>
      {id === undefined ? (
        <PlatformGrid platforms={platforms.data} />
      ) : chosen ? (
        <ConnectPlatform platform={chosen} />
      ) : (
        <EmptyState>{t('accounts:connectPage.unknownPlatform')}</EmptyState>
      )}
    </div>
  )
}

function PlatformGrid({ platforms }: { platforms: Platform[] }) {
  const { t } = useTranslation('accounts')
  const oauth = useOAuthRedirect()
  if (platforms.length === 0) return <EmptyState>{t('connectPage.none')}</EmptyState>
  return (
    <div className="flex flex-col gap-3">
      <ul className="grid gap-3 sm:grid-cols-2">
        {platforms.map((p) => (
          <li
            key={p.platform}
            className="flex items-center justify-between gap-3 rounded-xl border border-line bg-surface p-3"
          >
            <span className="flex flex-col gap-0.5">
              <PlatformBadge platform={p} />
              <span className="text-xs text-muted-foreground">
                {t(p.connect.method === 'oauth' ? 'connectPage.viaOauth' : 'connectPage.viaCredentials')}
              </span>
            </span>
            {p.connect.method === 'oauth' ? (
              <Button
                size="sm"
                disabled={oauth.pending}
                onClick={() => {
                  oauth.begin(p.platform)
                }}
              >
                {t('connectPage.connect')}
              </Button>
            ) : (
              <Button asChild size="sm">
                <Link to={`/accounts/connect/${p.platform}`}>{t('connectPage.connect')}</Link>
              </Button>
            )}
          </li>
        ))}
      </ul>
      <ErrorText error={oauth.error ? messageFor(oauth.error) : null} />
    </div>
  )
}

function ConnectPlatform({ platform }: { platform: Platform }) {
  const { t } = useTranslation('accounts')
  const me = useMe()
  const navigate = useNavigate()
  const oauth = useOAuthRedirect()
  const [checked, setChecked] = useState<CredentialCheck | null>(null)

  if (platform.connect.method === 'oauth') {
    return (
      <div className="flex max-w-lg flex-col gap-3">
        <PlatformBadge platform={platform} size="lg" />
        <p className="text-sm text-muted-foreground">
          {t('connectPage.oauthBody', { platform: platform.display_name })}
        </p>
        <ErrorText error={oauth.error ? messageFor(oauth.error) : null} />
        <Button
          className="self-start"
          disabled={oauth.pending}
          onClick={() => {
            oauth.begin(platform.platform)
          }}
        >
          {t('connectPage.oauthButton', { platform: platform.display_name })}
        </Button>
      </div>
    )
  }
  if (checked?.connect_id && checked.expires_at) {
    return (
      <DiscoveryChecklist
        connectId={checked.connect_id}
        pendingId={checked.pending_id ?? null}
        discovered={checked.discovered}
        expiresAt={checked.expires_at}
        onDone={(result) => {
          if (result !== 'connected') void navigate('/accounts')
        }}
      />
    )
  }
  return (
    <div className="flex flex-col gap-4">
      <PlatformBadge platform={platform} size="lg" />
      <CredentialsStep
        platform={platform}
        onChecked={(check) => {
          if (check.connect_id && check.expires_at && me.data) {
            savePending({
              connectId: check.connect_id,
              pendingId: check.pending_id ?? null,
              platform: platform.platform,
              userId: me.data.id,
              expiresAt: check.expires_at,
            })
          }
          setChecked(check)
        }}
      />
    </div>
  )
}
