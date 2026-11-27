import { Copy } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { DataTable } from '@/components/kit/DataTable'
import { ErrorText } from '@/components/kit/ErrorText'
import { PageHeader } from '@/components/kit/PageHeader'
import { PlatformBadge } from '@/components/kit/PlatformBadge'
import { columnHelper } from '@/components/kit/tableColumns'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { usePlatforms } from '@/features/platforms/queries'
import { useAdminPlatforms, useSetPlatformEnabled, type AdminPlatform } from './queries'

/** Every platform this build knows, whether the deployment configured it, and the switch that offers it to people. */
export function PlatformsPage() {
  const { t } = useTranslation(['admin', 'common'])
  const platforms = useAdminPlatforms()
  // The user-facing list holds the ones people can connect: those get their icon and brand color.
  const offered = usePlatforms()
  const setEnabled = useSetPlatformEnabled()
  const [copied, setCopied] = useState<string | null>(null)

  const copy = (id: string, uri: string) => {
    void navigator.clipboard.writeText(uri).then(() => {
      setCopied(id)
    })
  }

  const columns = useMemo(() => {
    const col = columnHelper<AdminPlatform>()
    return col.columns([
      col.display({
        id: 'name',
        header: t('admin:platforms.col.name'),
        cell: ({ row: { original: p } }) => {
          const listed = offered.data?.find((o) => o.platform === p.id)
          return listed ? <PlatformBadge platform={listed} size="sm" /> : <span dir="auto">{p.name}</span>
        },
      }),
      col.display({
        id: 'compiled',
        header: t('admin:platforms.col.compiled'),
        cell: ({ row: { original: p } }) => (p.compiled_in ? t('admin:yes') : t('admin:no')),
      }),
      col.display({
        id: 'configured',
        header: t('admin:platforms.col.configured'),
        cell: ({ row: { original: p } }) => (p.configured ? t('admin:yes') : t('admin:no')),
      }),
      col.display({
        id: 'enabled',
        header: t('admin:platforms.col.enabled'),
        cell: ({ row: { original: p } }) => (
          <Switch
            aria-label={t('admin:platforms.enable', { name: p.name })}
            checked={p.enabled}
            disabled={setEnabled.isPending}
            onCheckedChange={(enabled) => {
              setEnabled.mutate({ id: p.id, enabled })
            }}
          />
        ),
      }),
      col.display({
        id: 'redirect',
        header: t('admin:platforms.col.redirect'),
        meta: { className: 'whitespace-normal' },
        cell: ({ row: { original: p } }) =>
          p.redirect_uri ? (
            <span className="flex items-center gap-2">
              <code dir="ltr" className="break-all text-xs">
                {p.redirect_uri}
              </code>
              <Button
                variant="outline"
                size="sm"
                aria-label={t('admin:platforms.copy', { name: p.name })}
                onClick={() => {
                  copy(p.id, p.redirect_uri ?? '')
                }}
              >
                <Copy aria-hidden="true" />
                {copied === p.id ? t('admin:platforms.copied') : t('admin:platforms.copyLabel')}
              </Button>
            </span>
          ) : null,
      }),
    ])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, offered.data, setEnabled.isPending, copied])

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('admin:eyebrow')} title={t('admin:platforms.title')}>
        <p className="text-sm text-muted-foreground">{t('admin:platforms.intro')}</p>
      </PageHeader>
      {platforms.isError && <ErrorText error={platforms.error} />}
      <ErrorText error={setEnabled.error ? messageFor(setEnabled.error) : null} />
      <DataTable columns={columns} data={platforms.data} getRowId={(p) => p.id} label={t('admin:platforms.title')} />
    </div>
  )
}
