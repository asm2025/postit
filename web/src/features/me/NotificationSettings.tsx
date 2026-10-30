import { useId, useMemo, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { DataTable } from '@/components/kit/DataTable'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { columnHelper } from '@/components/kit/tableColumns'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { components } from '@/api/schema'
import { useKindName } from './notificationKinds'
import { useBrowserPush, type PushState } from './useBrowserPush'
import { usePreferences, useUpdatePreferences } from './usePreferences'

type KindSwitch = components['schemas']['KindSwitchDto']

const DEFAULT_ZONE = 'UTC'

/** Every IANA zone the browser knows, plus the ones in use (an engine's list may leave out UTC). */
function zoneOptions(...extra: string[]): { value: string; label: string; dir: 'ltr' }[] {
  const all = new Set<string>([DEFAULT_ZONE, ...Intl.supportedValuesOf('timeZone'), ...extra])
  return [...all].sort().map((z) => ({ value: z, label: z, dir: 'ltr' }))
}

function browserZone(): string | null {
  try {
    return new Intl.DateTimeFormat().resolvedOptions().timeZone || null
  } catch {
    return null
  }
}

/** A switch with its label and explanation, the switch at the end of the row. */
function SettingRow({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string
  hint: ReactNode
  checked: boolean
  disabled?: boolean
  onChange: (value: boolean) => void
}) {
  const id = useId()
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="flex min-w-0 flex-col gap-1">
        <Label htmlFor={id}>{label}</Label>
        <p id={`${id}-hint`} className="text-sm text-muted-foreground">
          {hint}
        </p>
      </div>
      <Switch
        id={id}
        aria-describedby={`${id}-hint`}
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
      />
    </div>
  )
}

function BrowserPushRow() {
  const { t } = useTranslation('me')
  const push = useBrowserPush()
  const text: Record<PushState, string> = {
    on: t('notifications.push.on'),
    off: t('notifications.push.off'),
    working: t('notifications.push.working'),
    blocked: t('notifications.push.blocked'),
    unsupported: push.serverOff ? t('notifications.push.serverOff') : t('notifications.push.unsupported'),
  }
  return (
    <div className="flex flex-col gap-1">
      <SettingRow
        label={t('notifications.push.label')}
        hint={text[push.state]}
        checked={push.state === 'on'}
        disabled={push.state !== 'on' && push.state !== 'off'}
        onChange={(on) => {
          void (on ? push.enable() : push.disable())
        }}
      />
      {push.error != null && <ErrorText error={t('notifications.push.failed')} />}
    </div>
  )
}

function KindTable({
  switches,
  onChange,
}: {
  switches: readonly KindSwitch[]
  onChange: (kind: KindSwitch['kind'], change: { push: boolean } | { email: boolean }) => void
}) {
  const { t } = useTranslation('me')
  const kindName = useKindName()
  const col = columnHelper<KindSwitch>()
  const columns = col.columns([
    col.display({
      id: 'kind',
      header: t('notifications.kinds.kind'),
      meta: { className: 'whitespace-normal' },
      cell: ({ row }) => kindName(row.original.kind),
    }),
    col.display({
      id: 'push',
      header: t('notifications.kinds.push'),
      cell: ({ row: { original: s } }) => (
        <Switch
          aria-label={t('notifications.kinds.pushFor', { kind: kindName(s.kind) })}
          checked={s.push}
          onCheckedChange={(push) => {
            onChange(s.kind, { push })
          }}
        />
      ),
    }),
    col.display({
      id: 'email',
      header: t('notifications.kinds.email'),
      cell: ({ row: { original: s } }) =>
        s.email == null ? (
          <span aria-hidden="true" className="text-faint">
            —
          </span>
        ) : (
          <Switch
            aria-label={t('notifications.kinds.emailFor', { kind: kindName(s.kind) })}
            checked={s.email}
            onCheckedChange={(email) => {
              onChange(s.kind, { email })
            }}
          />
        ),
    }),
  ])
  return (
    <DataTable columns={columns} data={[...switches]} getRowId={(s) => s.kind} label={t('notifications.kinds.table')} />
  )
}

/** Browser push, email, digest, time zone, and the per-kind table, on the profile page. */
export function NotificationSettings() {
  const { t } = useTranslation('me')
  const prefs = usePreferences()
  const update = useUpdatePreferences()
  const save = update.mutate
  const data = prefs.data
  const options = useMemo(() => zoneOptions(...(data ? [data.timezone] : [])), [data])
  const detected = browserZone()
  // Still on the default, the browser's zone is the likelier truth: shown, and saved when chosen.
  const prefilled = data?.timezone === DEFAULT_ZONE && detected !== null && detected !== DEFAULT_ZONE
  const zone = data ? (prefilled ? detected : data.timezone) : ''

  const changeKind = useMemo(
    () => (kind: KindSwitch['kind'], change: { push: boolean } | { email: boolean }) => {
      save({ switches: [{ kind, ...change }] })
    },
    [save],
  )

  return (
    <section aria-labelledby="notifications-title" className="flex flex-col gap-5">
      <div className="flex flex-col gap-1">
        <h2 id="notifications-title" className="font-display text-xl font-semibold">
          {t('notifications.title')}
        </h2>
        <p className="text-sm text-muted-foreground">{t('notifications.intro')}</p>
      </div>
      <BrowserPushRow />
      {prefs.isError && <ErrorText error={prefs.error} />}
      {data && (
        <>
          <SettingRow
            label={t('notifications.email.label')}
            hint={t('notifications.email.hint')}
            checked={data.email_notifications}
            onChange={(email_notifications) => {
              save({ email_notifications })
            }}
          />
          <SettingRow
            label={t('notifications.digest.label')}
            hint={t('notifications.digest.hint')}
            checked={data.comment_digest}
            onChange={(comment_digest) => {
              save({ comment_digest })
            }}
          />
          <div className="flex flex-col gap-1.5">
            <FormField label={t('notifications.timezone.label')}>
              <OptionSelect
                aria-label={t('notifications.timezone.label')}
                value={zone}
                options={options}
                onChange={(timezone) => {
                  save({ timezone })
                }}
              />
            </FormField>
            {prefilled ? (
              <div className="flex flex-wrap items-center gap-3">
                <p className="text-sm text-muted-foreground">{t('notifications.timezone.detected')}</p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    save({ timezone: zone })
                  }}
                >
                  {t('notifications.timezone.save', { zone })}
                </Button>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t('notifications.timezone.hint')}</p>
            )}
          </div>
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-medium">{t('notifications.kinds.heading')}</h3>
            <KindTable switches={data.switches} onChange={changeKind} />
          </div>
          <ErrorText error={update.error} />
        </>
      )}
    </section>
  )
}
