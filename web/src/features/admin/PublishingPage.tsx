import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { Switch } from '@/components/ui/switch'
import { useFormat } from '@/i18n/useFormat'
import { usePublishingSettings, useSetLive } from './queries'

/** The live-publishing switch. Turning it off parks live deliveries, but not what platforms already hold. */
export function PublishingPage() {
  const { t } = useTranslation(['admin', 'common'])
  const { number } = useFormat()
  const settings = usePublishingSettings()
  const setLive = useSetLive()
  const [confirmOff, setConfirmOff] = useState(false)
  const data = settings.data

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('admin:eyebrow')} title={t('admin:publishing.title')}>
        <p className="text-sm text-muted-foreground">{t('admin:publishing.intro')}</p>
      </PageHeader>
      {settings.isError && <ErrorText error={settings.error} />}
      {data && (
        <section className="flex max-w-lg flex-col gap-3">
          <label className="flex items-center justify-between gap-4 text-sm">
            <span>
              <span className="font-medium">{t('admin:publishing.live')}</span>
              <span className="block text-muted-foreground">
                {data.live ? t('admin:publishing.liveOn') : t('admin:publishing.liveOff')}
              </span>
            </span>
            <Switch
              checked={data.live}
              disabled={setLive.isPending}
              onCheckedChange={(live) => {
                if (live) setLive.mutate(true)
                else setConfirmOff(true)
              }}
            />
          </label>
          <p className="text-sm text-muted-foreground">
            {t('admin:publishing.native', {
              count: data.native_scheduled_count,
              formatted: number(data.native_scheduled_count),
            })}
          </p>
          <ErrorText error={setLive.error ? messageFor(setLive.error) : null} />
        </section>
      )}
      <ConfirmDialog
        open={confirmOff}
        title={t('admin:publishing.offTitle')}
        description={
          <>
            <span className="block">{t('admin:publishing.offBody')}</span>
            {data && data.native_scheduled_count > 0 && (
              <span className="mt-2 block">
                {t('admin:publishing.nativeWarning', {
                  count: data.native_scheduled_count,
                  formatted: number(data.native_scheduled_count),
                })}
              </span>
            )}
          </>
        }
        confirmLabel={t('admin:publishing.turnOff')}
        destructive
        busy={setLive.isPending}
        onCancel={() => {
          setConfirmOff(false)
        }}
        onConfirm={() => {
          setLive.mutate(false, {
            onSettled: () => {
              setConfirmOff(false)
            },
          })
        }}
      />
    </div>
  )
}
