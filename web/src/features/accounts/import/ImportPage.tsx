import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useSearchParams } from 'react-router'
import { ApiError, messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { PageHeader } from '@/components/kit/PageHeader'
import { ProgressBar } from '@/components/kit/ProgressBar'
import { Button } from '@/components/ui/button'
import { parseSchema } from '@/components/kit/schema'
import { usePlatforms } from '@/features/platforms/queries'
import { useFormat } from '@/i18n/useFormat'
import { ImportReport } from './ImportReport'
import { useCancelImport, useContinueImport, useImport, useStartImport, type AccountImport } from './queries'

const DISPLAY_NAME = 'display_name'

/** Checks a file of credentials with the platform, then connects the valid rows. */
export function ImportPage() {
  const { t } = useTranslation(['accounts', 'common'])
  const { number } = useFormat()
  const [params, setParams] = useSearchParams()
  const importId = params.get('import')
  const platforms = usePlatforms()
  const current = useImport(importId)
  const start = useStartImport()
  const proceed = useContinueImport()
  const cancel = useCancelImport()
  const input = useRef<HTMLInputElement>(null)
  const [platformChoice, setPlatformChoice] = useState('')
  const [file, setFile] = useState<File | null>(null)
  const [confirmCancel, setConfirmCancel] = useState(false)

  const importDto = current.data
  const credentialPlatforms = (platforms.data ?? []).filter((p) => p.connect.method === 'credentials')
  // A reloaded page knows its import, not the file or the choice made before.
  const platform = importDto?.platform ?? platformChoice
  const descriptor = credentialPlatforms.find((p) => p.platform === platform)
  const columns =
    descriptor?.connect.method === 'credentials' ? parseSchema(descriptor.connect.schema).map((f) => f.name) : []

  const show = (id: string) => {
    const next = new URLSearchParams(params)
    next.set('import', id)
    next.delete('page')
    setParams(next, { replace: true })
  }
  const run = (commit: boolean) => {
    if (!file || !platform) return
    start.mutate(
      { platform, file, commit },
      {
        onSuccess: (started) => {
          show(started.id)
        },
      },
    )
  }
  const reload = () => {
    void current.refetch()
  }
  // The server says the import moved on while the page waited: show where it stands.
  const raced = (error: unknown) => error instanceof ApiError && error.code === 'import_not_waiting'
  const actionError = [proceed.error, cancel.error].find(Boolean)
  const error =
    start.error ?? (actionError && !raced(actionError) ? actionError : null) ?? (current.isError ? current.error : null)

  const template =
    columns.length > 0
      ? `data:text/csv;charset=utf-8,${encodeURIComponent([...columns, DISPLAY_NAME].join(','))}%0A`
      : ''
  const isDryRun = importDto !== undefined && !importDto.commit
  const busy = start.isPending || proceed.isPending || cancel.isPending

  return (
    <div className="flex flex-1 flex-col gap-6">
      <PageHeader eyebrow={t('accounts:title')} title={t('accounts:import.title')}>
        <p className="text-sm text-muted-foreground">{t('accounts:import.intro')}</p>
        <Button asChild variant="outline" size="sm">
          <Link to="/accounts">{t('accounts:back')}</Link>
        </Button>
      </PageHeader>

      <section className="flex max-w-lg flex-col gap-4">
        <FormField label={t('accounts:import.platform')}>
          <OptionSelect
            aria-label={t('accounts:import.platform')}
            value={platform}
            disabled={importDto !== undefined}
            options={credentialPlatforms.map((p) => ({ value: p.platform, label: p.display_name }))}
            onChange={setPlatformChoice}
          />
        </FormField>
        {columns.length > 0 && (
          <div className="flex flex-col gap-1 text-sm">
            <p>
              {t('accounts:import.columns')} <code dir="ltr">{columns.join(', ')}</code>
              {t('accounts:import.columnsOptional', { column: DISPLAY_NAME })}
            </p>
            <a className="underline" href={template} download={`${platform}-accounts.csv`}>
              {t('accounts:import.template')}
            </a>
          </div>
        )}
        <FormField label={t('accounts:import.file')}>
          <input
            ref={input}
            type="file"
            accept=".csv,text/csv"
            className="text-sm"
            onChange={(e) => {
              setFile(e.target.files?.[0] ?? null)
            }}
          />
        </FormField>
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={!file || !platform || busy}
            onClick={() => {
              run(false)
            }}
          >
            {t('accounts:import.check')}
          </Button>
          {isDryRun &&
            importDto.status === 'completed' &&
            (file ? (
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => {
                  run(true)
                }}
              >
                {t('accounts:import.connectValid')}
              </Button>
            ) : (
              <Button
                variant="outline"
                onClick={() => {
                  input.current?.click()
                }}
              >
                {t('accounts:import.pickAgain')}
              </Button>
            ))}
        </div>
        <ErrorText error={error ? messageFor(error) : null} />
      </section>

      {importDto && (
        <ImportState
          importDto={importDto}
          busy={busy}
          onContinue={() => {
            proceed.mutate(importDto.id, { onError: reload })
          }}
          onAskCancel={() => {
            setConfirmCancel(true)
          }}
          number={number}
        />
      )}
      {importDto && importDto.rows.length > 0 && <ImportReport importDto={importDto} />}
      <ConfirmDialog
        open={confirmCancel}
        title={t('accounts:import.cancelTitle')}
        description={t('accounts:import.cancelBody')}
        confirmLabel={t('accounts:import.cancelButton')}
        destructive
        busy={cancel.isPending}
        onCancel={() => {
          setConfirmCancel(false)
        }}
        onConfirm={() => {
          if (!importDto) return
          cancel.mutate(importDto.id, {
            onSettled: () => {
              setConfirmCancel(false)
            },
            onError: reload,
          })
        }}
      />
    </div>
  )
}

function ImportState({
  importDto,
  busy,
  onContinue,
  onAskCancel,
  number,
}: {
  importDto: AccountImport
  busy: boolean
  onContinue: () => void
  onAskCancel: () => void
  number: (n: number) => string
}) {
  const { t } = useTranslation('accounts')
  const reason = importDto.stopped ? t(`importStopped.${importDto.stopped}`) : null
  return (
    <section aria-label={t('import.state')} className="flex max-w-2xl flex-col gap-3">
      {importDto.status === 'processing' && (
        <>
          <ProgressBar value={importDto.rows_done} max={importDto.rows_total} label={t('import.progress')} />
          <p className="text-sm text-muted-foreground">
            {t('import.progressText', { done: number(importDto.rows_done), total: number(importDto.rows_total) })}
          </p>
        </>
      )}
      {reason && importDto.status !== 'awaiting_decision' && (
        <p role="status" className="rounded-xl border border-line bg-raised px-4 py-2 text-sm">
          {importDto.commit
            ? t('import.stoppedCommit', { line: importDto.last_line, reason })
            : t('import.stoppedCheck', { line: importDto.last_line, reason })}
        </p>
      )}
      {importDto.status === 'awaiting_decision' && (
        <div role="status" className="flex flex-col gap-3 rounded-xl border border-line bg-raised px-4 py-3 text-sm">
          <p>{t('import.waiting', { line: importDto.last_line, reason: reason ?? '' })}</p>
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy} onClick={onContinue}>
              {t('import.connectRead', { count: importDto.rows_total })}
            </Button>
            <Button variant="outline" disabled={busy} onClick={onAskCancel}>
              {t('import.cancelButton')}
            </Button>
          </div>
          <p className="text-muted-foreground">{t('import.waitingExpiry')}</p>
        </div>
      )}
      {importDto.status === 'completed' && importDto.commit && importDto.stopped && (
        <p role="status" className="text-sm">
          {t('import.removeLines', { line: importDto.last_line })}
        </p>
      )}
      {importDto.status === 'canceled' && (
        <p role="status" className="text-sm">
          {t('import.canceled')}
        </p>
      )}
      {importDto.status === 'failed' && (
        <p role="alert" className="text-sm text-destructive">
          {t('import.failed')}
        </p>
      )}
    </section>
  )
}
