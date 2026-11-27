import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { Button } from '@/components/ui/button'
import { usePreferences, useUpdatePreferences } from './usePreferences'

const DEFAULT_ZONE = 'UTC'
/** A per-browser convenience, not a secret: whether the user was already asked about this browser's zone. */
const TZ_ASKED_KEY = 'postit.tzAsked'

/** Every IANA zone the browser knows, plus the ones in use (an engine's list may leave out UTC). */
function zoneOptions(...extra: string[]): { value: string; label: string; dir: 'ltr' }[] {
  let known: string[] = []
  try {
    // Older Safari and Firefox lack it: the zones in use are then the whole list.
    known = Intl.supportedValuesOf('timeZone')
  } catch {
    /* keep the fallback */
  }
  const all = new Set<string>([DEFAULT_ZONE, ...known, ...extra])
  return [...all].sort().map((z) => ({ value: z, label: z, dir: 'ltr' }))
}

function browserZone(): string | null {
  try {
    return new Intl.DateTimeFormat().resolvedOptions().timeZone || null
  } catch {
    return null
  }
}

function alreadyAsked(): boolean {
  try {
    return localStorage.getItem(TZ_ASKED_KEY) === '1'
  } catch {
    return false
  }
}

function markAsked() {
  try {
    localStorage.setItem(TZ_ASKED_KEY, '1')
  } catch {
    // Storage blocked: the question may come back, which is harmless.
  }
}

/**
 * The user's time zone: a picker, the browser's zone offered while the account still has the
 * default, and a one-time question when a saved zone differs from the browser's.
 */
export function TimeZoneSetting() {
  const { t } = useTranslation('me')
  const prefs = usePreferences()
  const update = useUpdatePreferences()
  const save = update.mutate
  const data = prefs.data
  const detected = browserZone()
  const [asked, setAsked] = useState(alreadyAsked)
  // The detected zone may be an alias the engine's list leaves out (Asia/Calcutta); it must still show.
  const options = useMemo(
    () => zoneOptions(...(data ? [data.timezone] : []), ...(detected ? [detected] : [])),
    [data, detected],
  )
  if (!data) return null
  // Still on the default, the browser's zone is the likelier truth: shown, and saved when chosen.
  const prefilled = data.timezone === DEFAULT_ZONE && detected !== null && detected !== DEFAULT_ZONE
  const zone = prefilled ? detected : data.timezone
  // A zone the user saved on purpose differs from this browser's: ask once, and take either answer.
  const ask = !asked && !prefilled && detected !== null && detected !== data.timezone

  const answer = (useBrowser: boolean) => {
    markAsked()
    setAsked(true)
    if (useBrowser && detected) save({ timezone: detected })
  }

  return (
    <div className="flex flex-col gap-1.5">
      {ask && (
        <div
          role="group"
          aria-label={t('notifications.timezone.askLabel')}
          className="flex flex-col gap-2 rounded-xl border border-line bg-raised p-3"
        >
          <p className="text-sm">{t('notifications.timezone.ask', { zone: detected, saved: data.timezone })}</p>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              onClick={() => {
                answer(true)
              }}
            >
              {t('notifications.timezone.useBrowser', { zone: detected })}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                answer(false)
              }}
            >
              {t('notifications.timezone.keep', { zone: data.timezone })}
            </Button>
          </div>
        </div>
      )}
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
      <ErrorText error={update.error} />
    </div>
  )
}
