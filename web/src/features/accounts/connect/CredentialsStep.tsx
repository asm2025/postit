import i18next from 'i18next'
import { useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, messageFor } from '@/api/errors'
import { ErrorText } from '@/components/kit/ErrorText'
import { initialValues, parseSchema, submittedValues } from '@/components/kit/schema'
import { SchemaForm } from '@/components/kit/SchemaForm'
import { Button } from '@/components/ui/button'
import type { Platform } from '@/features/platforms/queries'
import { agentText } from '@/i18n/agentText'
import { useStartCredentials, type CredentialCheck } from './queries'

const translate = i18next.t.bind(i18next) as (key: string, params?: object) => string

/** The form field a violation points at: `options.<field>` names it. */
const fieldOf = (path: string | null | undefined) =>
  path?.startsWith('options.') ? path.slice('options.'.length) : null

/** The guide, then the platform's credential form; the platform's own words come from its descriptor. */
export function CredentialsStep({
  platform,
  onChecked,
}: {
  platform: Platform
  onChecked: (check: CredentialCheck) => void
}) {
  const { t } = useTranslation(['accounts', 'violations'])
  const start = useStartCredentials(platform.platform)
  const fields = useMemo(
    () => (platform.connect.method === 'credentials' ? parseSchema(platform.connect.schema) : []),
    [platform],
  )
  const [values, setValues] = useState(() => initialValues(fields))
  const [empty, setEmpty] = useState(false)

  const errors: Record<string, ReactNode> = {}
  let general: unknown = null
  if (start.error instanceof ApiError) {
    const error = start.error
    if (error.code === 'validation_failed') {
      for (const v of error.violations) {
        const field = fieldOf(v.path)
        const text = translate(`violations:${v.code}`, v.params ?? {})
        if (field && !errors[field]) errors[field] = text
        else general = general ?? text
      }
      if (error.violations.length === 0) general = messageFor(error)
    } else if (error.code === 'credentials_invalid' && error.field) {
      errors[error.field] = t('accounts:credentials.invalid')
    } else {
      general = messageFor(error)
    }
  } else if (start.error) {
    general = messageFor(start.error)
  }

  return (
    <form
      className="flex max-w-lg flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault()
        setEmpty(false)
        start.mutate(submittedValues(fields, values), {
          onSuccess: (check) => {
            if (check.connect_id) onChecked(check)
            else setEmpty(true)
          },
        })
      }}
    >
      {platform.guide.steps.length > 0 && (
        <section aria-labelledby="guide-title" className="flex flex-col gap-2">
          <h2 id="guide-title" className="text-base font-semibold">
            {t('accounts:credentials.guide')}
          </h2>
          <ol className="list-decimal space-y-1 ps-5 text-sm">
            {platform.guide.steps.map((step) => (
              <li key={step.key}>{agentText(step)}</li>
            ))}
          </ol>
          {platform.guide.url && (
            <a className="text-sm underline" href={platform.guide.url} target="_blank" rel="noreferrer">
              {t('accounts:credentials.moreHelp')}
            </a>
          )}
        </section>
      )}
      <SchemaForm fields={fields} values={values} onChange={setValues} errors={errors} disabled={start.isPending} />
      <ErrorText error={general === null ? null : typeof general === 'string' ? general : messageFor(general)} />
      {empty && (
        <p role="alert" className="text-sm text-destructive">
          {t('accounts:credentials.noAccounts')}
        </p>
      )}
      <Button type="submit" className="self-start" disabled={start.isPending}>
        {t('accounts:credentials.check')}
      </Button>
    </form>
  )
}
