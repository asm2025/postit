import { Eye, EyeOff } from 'lucide-react'
import { useId, useState, type ComponentProps, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { agentText } from '@/i18n/agentText'
import { fromLocalInput, toLocalInput } from './datetime'
import { OptionSelect } from './OptionSelect'
import { isVisible, type SchemaFieldSpec } from './schema'

interface Props {
  fields: SchemaFieldSpec[]
  values: Record<string, unknown>
  onChange: (next: Record<string, unknown>) => void
  /** A message per field name, shown under its control. */
  errors?: Record<string, ReactNode>
  disabled?: boolean
}

/** Draws any agent's options schema; knows no platform. */
export function SchemaForm({ fields, values, onChange, errors, disabled }: Props) {
  const set = (name: string, value: unknown) => {
    onChange({ ...values, [name]: value })
  }
  return (
    <div className="flex flex-col gap-4">
      {fields
        .filter((f) => isVisible(f, values))
        .map((f) => (
          <Field key={f.name} field={f} value={values[f.name]} error={errors?.[f.name]} disabled={disabled} set={set} />
        ))}
    </div>
  )
}

function Field({
  field,
  value,
  error,
  disabled,
  set,
}: {
  field: SchemaFieldSpec
  value: unknown
  error: ReactNode
  disabled?: boolean
  set: (name: string, value: unknown) => void
}) {
  const { t } = useTranslation()
  const id = useId()
  const helpId = `${id}-help`
  const errorId = `${id}-error`
  const label = agentText(field.label)
  const describedBy = [field.help ? helpId : '', error ? errorId : ''].filter(Boolean).join(' ') || undefined
  const common = { id, disabled, 'aria-invalid': error ? true : undefined, 'aria-describedby': describedBy }
  const text = typeof value === 'string' ? value : ''
  const k = field.kind

  let control: ReactNode
  if (k.type === 'boolean' && field.consentText) {
    // A consent box carries its own sentence and is never optional.
    return (
      <div className="flex flex-col gap-1">
        <div className="flex items-start gap-2.5">
          <Checkbox
            {...common}
            required
            checked={value === true}
            onCheckedChange={(checked) => {
              set(field.name, checked === true)
            }}
            className="mt-0.5"
          />
          <label htmlFor={id} className="text-sm">
            {agentText(field.consentText)}
          </label>
        </div>
        <Messages help={field.help} helpId={helpId} error={error} errorId={errorId} />
      </div>
    )
  }
  switch (k.type) {
    case 'string':
      control = field.secret ? (
        <SecretInput
          {...common}
          value={text}
          onChange={(v) => {
            set(field.name, v)
          }}
          required={field.required}
          maxLength={k.maxLength}
        />
      ) : (
        <Input
          {...common}
          type="text"
          value={text}
          required={field.required}
          minLength={k.minLength}
          maxLength={k.maxLength}
          onChange={(e) => {
            set(field.name, e.target.value)
          }}
        />
      )
      break
    case 'multiline':
      control = (
        <Textarea
          {...common}
          value={text}
          required={field.required}
          maxLength={k.maxLength}
          onChange={(e) => {
            set(field.name, e.target.value)
          }}
        />
      )
      break
    case 'enum':
      control = (
        <OptionSelect
          aria-label={label}
          value={text}
          disabled={disabled}
          options={k.values.map((v) => ({ value: v.value, label: agentText(v.label) }))}
          anyLabel={field.required ? undefined : t('schema.unset')}
          onChange={(v) => {
            set(field.name, v === '' ? undefined : v)
          }}
        />
      )
      break
    case 'boolean':
      control = (
        <Switch
          {...common}
          checked={value === true}
          onCheckedChange={(checked) => {
            set(field.name, checked)
          }}
        />
      )
      break
    case 'integer':
      control = (
        <Input
          {...common}
          type="number"
          inputMode="numeric"
          value={typeof value === 'number' ? String(value) : ''}
          required={field.required}
          min={k.minimum}
          max={k.maximum}
          onChange={(e) => {
            const n = e.target.valueAsNumber
            set(field.name, Number.isNaN(n) ? undefined : n)
          }}
        />
      )
      break
    case 'datetime':
      control = (
        <Input
          {...common}
          type="datetime-local"
          value={typeof value === 'string' ? toLocalInput(value) : ''}
          required={field.required}
          onChange={(e) => {
            set(field.name, e.target.value ? fromLocalInput(e.target.value) : undefined)
          }}
        />
      )
      break
  }

  const inline = k.type === 'boolean'
  return (
    <div className="flex flex-col gap-1.5">
      <div className={inline ? 'flex items-center justify-between gap-3' : 'flex flex-col gap-1.5'}>
        <label htmlFor={id} className="text-sm font-medium">
          {label}
          {field.required && (
            <span aria-hidden="true" className="ms-0.5 text-destructive">
              *
            </span>
          )}
        </label>
        {control}
      </div>
      <Messages help={field.help} helpId={helpId} error={error} errorId={errorId} />
    </div>
  )
}

function Messages({
  help,
  helpId,
  error,
  errorId,
}: {
  help: SchemaFieldSpec['help']
  helpId: string
  error: ReactNode
  errorId: string
}) {
  return (
    <>
      {help && (
        <p id={helpId} className="text-xs text-muted-foreground">
          {agentText(help)}
        </p>
      )}
      {error ? (
        <p id={errorId} role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </>
  )
}

/** A password-style input that is never filled from outside, with a button to see what was typed. */
function SecretInput({
  value,
  onChange,
  ...rest
}: { value: string; onChange: (value: string) => void } & Omit<ComponentProps<'input'>, 'value' | 'onChange'>) {
  const { t } = useTranslation()
  const [shown, setShown] = useState(false)
  const Icon = shown ? EyeOff : Eye
  return (
    <div className="relative">
      <Input
        {...rest}
        type={shown ? 'text' : 'password'}
        autoComplete="off"
        value={value}
        className="pe-11"
        dir="ltr"
        onChange={(e) => {
          onChange(e.target.value)
        }}
      />
      <button
        type="button"
        aria-label={shown ? t('schema.hide') : t('schema.show')}
        aria-pressed={shown}
        disabled={rest.disabled}
        onClick={() => {
          setShown((s) => !s)
        }}
        className="absolute inset-y-0 end-0 flex size-11 cursor-pointer items-center justify-center text-muted-foreground"
      >
        <Icon className="size-4" aria-hidden="true" />
      </button>
    </div>
  )
}
