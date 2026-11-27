import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useParams } from 'react-router'
import { ApiError, messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { DangerZone } from '@/components/kit/DangerZone'
import { EmptyState } from '@/components/kit/EmptyState'
import { ErrorText } from '@/components/kit/ErrorText'
import { FactList } from '@/components/kit/FactList'
import { FormField } from '@/components/kit/FormField'
import { PageHeader } from '@/components/kit/PageHeader'
import { PlatformBadge } from '@/components/kit/PlatformBadge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { usePlatform } from '@/features/platforms/queries'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { useAccess } from '@/workspace/useAccess'
import { AccountStatus } from './AccountStatus'
import { useAccount, useAccountActions, useWritingStyle, type Account } from './queries'
import { WritingStyleForm } from './WritingStyleForm'

/** One account: its facts, the owner's actions on it, and how the drafter writes for it. */
export function AccountDetailPage() {
  const { t } = useTranslation(['accounts', 'common'])
  const { id = '' } = useParams()
  const account = useAccount(id)
  if (account.isPending) return <p className="text-muted-foreground">{t('common:loading')}</p>
  if (account.isError) {
    // An account that is gone, or outside a delegate's limit, looks the same.
    const notFound = account.error instanceof ApiError && account.error.status === 404
    return (
      <div className="flex flex-col items-start gap-3">
        <EmptyState>{notFound ? t('accounts:notFound') : messageFor(account.error)}</EmptyState>
        <Button asChild variant="outline">
          <Link to="/accounts">{t('accounts:back')}</Link>
        </Button>
      </div>
    )
  }
  return <Detail account={account.data} />
}

function Detail({ account }: { account: Account }) {
  const { t } = useTranslation(['accounts', 'common'])
  const { dateTime } = useFormat()
  const access = useAccess()
  const navigate = useNavigate()
  const platform = usePlatform(account.platform)
  const actions = useAccountActions(account.id)
  const style = useWritingStyle(account.id)
  const [name, setName] = useState(account.display_name)
  const [confirming, setConfirming] = useState(false)
  const checked = actions.check.data
  const styleEditable = access.atLeast('edit') && access.inLimit(account.id)

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('accounts:title')} title={<bdi dir="auto">{account.display_name}</bdi>} />
      <FactList
        facts={[
          [
            t('accounts:fact.platform'),
            platform ? <PlatformBadge platform={platform} size="sm" /> : <bdi dir="auto">{account.platform}</bdi>,
          ],
          [t('accounts:fact.remoteId'), <bdi dir="ltr">{account.remote_account_id}</bdi>],
          [t('accounts:fact.status'), <AccountStatus status={(checked ?? account).status} />],
          [t('accounts:fact.connected'), dateTime(account.created_at)],
          [t('accounts:fact.checked'), account.last_checked_at ? dateTime(account.last_checked_at) : '—'],
        ]}
      />

      {access.own && (
        <section aria-labelledby="manage-title" className="flex max-w-lg flex-col gap-4">
          <h2 id="manage-title" className="font-display text-xl font-semibold">
            {t('accounts:manage.title')}
          </h2>
          <FormField label={t('accounts:manage.name')}>
            <div className="flex gap-2">
              <Input
                value={name}
                dir="auto"
                maxLength={100}
                onChange={(e) => {
                  setName(e.target.value)
                }}
              />
              <Button
                variant="outline"
                disabled={actions.rename.isPending || name.trim() === '' || name === account.display_name}
                onClick={() => {
                  actions.rename.mutate(name.trim())
                }}
              >
                {t('common:actions.save')}
              </Button>
            </div>
          </FormField>
          <label className="flex items-center justify-between gap-4 text-sm">
            <span>
              <span className="font-medium">{t('accounts:manage.test')}</span>
              <span className="block text-muted-foreground">{t('accounts:manage.testHelp')}</span>
            </span>
            <Switch
              checked={account.test_account}
              disabled={actions.setTestAccount.isPending}
              onCheckedChange={(on) => {
                actions.setTestAccount.mutate(on)
              }}
            />
          </label>
          <div className="flex flex-wrap items-center gap-3">
            <Button
              variant="outline"
              disabled={actions.check.isPending || account.status === 'disconnected'}
              onClick={() => {
                actions.check.mutate()
              }}
            >
              {t('accounts:manage.check')}
            </Button>
            {checked && (
              <p role="status" className="text-sm text-muted-foreground">
                {t('accounts:manage.checkResult', { status: t(`accounts:status.${checked.status}`) })}
              </p>
            )}
          </div>
          <ErrorText
            error={
              (actions.rename.error ?? actions.setTestAccount.error ?? actions.check.error)
                ? messageFor(actions.rename.error ?? actions.setTestAccount.error ?? actions.check.error)
                : null
            }
          />
        </section>
      )}

      {style.isError ? (
        <ErrorText error={style.error} />
      ) : style.data ? (
        <WritingStyleForm accountId={account.id} style={style.data} editable={styleEditable} />
      ) : null}

      {access.own && account.status !== 'disconnected' && (
        <DangerZone pinned description={t('accounts:disconnect.body')}>
          <Button
            variant="destructive"
            onClick={() => {
              setConfirming(true)
            }}
          >
            {t('accounts:disconnect.button')}
          </Button>
        </DangerZone>
      )}
      <ConfirmDialog
        open={confirming}
        title={t('accounts:disconnect.title')}
        description={t('accounts:disconnect.confirm', { name: isolate(account.display_name) })}
        confirmLabel={t('accounts:disconnect.button')}
        destructive
        busy={actions.disconnect.isPending}
        onCancel={() => {
          setConfirming(false)
        }}
        onConfirm={() => {
          actions.disconnect.mutate(undefined, {
            onSuccess: () => {
              setConfirming(false)
              void navigate('/accounts')
            },
          })
        }}
      />
      <ErrorText error={actions.disconnect.error ? messageFor(actions.disconnect.error) : null} />
    </div>
  )
}
