import type { AgentMessage } from '@/i18n/agentText'

export type FieldKind =
  | { type: 'string'; minLength?: number; maxLength?: number }
  | { type: 'multiline'; maxLength?: number }
  | { type: 'enum'; values: { value: string; label: AgentMessage }[] }
  | { type: 'boolean' }
  | { type: 'integer'; minimum?: number; maximum?: number }
  | { type: 'datetime' }

export interface SchemaFieldSpec {
  name: string
  label: AgentMessage
  help?: AgentMessage
  required: boolean
  kind: FieldKind
  default?: unknown
  /** The user must choose: the form starts empty and the field counts as required. */
  noDefault: boolean
  secret: boolean
  /** A boolean with consent text is a consent box the user has to tick. */
  consentText?: AgentMessage
  dependsOn?: { field: string; equals: unknown }
}

/** The property keywords the form understands; a field with any other is skipped, never half-drawn. */
const KEYWORDS = new Set([
  'type',
  'format',
  'enum',
  'default',
  'minLength',
  'maxLength',
  'minimum',
  'maximum',
  'x-postit-label',
  'x-postit-help',
  'x-postit-enum-labels',
  'x-postit-multiline',
  'x-postit-no-default',
  'x-postit-secret',
  'x-postit-consent-text',
  'x-postit-depends-on',
])

type Raw = Record<string, unknown>

const isObject = (v: unknown): v is Raw => typeof v === 'object' && v !== null && !Array.isArray(v)
const num = (v: unknown): number | undefined => (typeof v === 'number' ? v : undefined)

function message(v: unknown): AgentMessage | undefined {
  if (!isObject(v) || typeof v.key !== 'string' || typeof v.en !== 'string') return undefined
  return { key: v.key, en: v.en }
}

function kindOf(raw: Raw): FieldKind | undefined {
  switch (raw.type) {
    case 'boolean':
      return { type: 'boolean' }
    case 'integer':
      return { type: 'integer', minimum: num(raw.minimum), maximum: num(raw.maximum) }
    case 'string': {
      if (raw.format === 'date-time') return { type: 'datetime' }
      if (Array.isArray(raw.enum)) {
        const labels = Array.isArray(raw['x-postit-enum-labels']) ? (raw['x-postit-enum-labels'] as unknown[]) : []
        const values = raw.enum.filter((v): v is string => typeof v === 'string')
        return {
          type: 'enum',
          values: values.map((value, i) => ({ value, label: message(labels[i]) ?? { key: value, en: value } })),
        }
      }
      if (raw['x-postit-multiline'] === true) return { type: 'multiline', maxLength: num(raw.maxLength) }
      return { type: 'string', minLength: num(raw.minLength), maxLength: num(raw.maxLength) }
    }
    default:
      return undefined
  }
}

/** The fields of an agent's options schema, in the order the agent declared them. */
export function parseSchema(schema: unknown): SchemaFieldSpec[] {
  if (!isObject(schema) || !isObject(schema.properties)) return []
  const required = new Set(Array.isArray(schema.required) ? (schema.required as unknown[]) : [])
  const fields: SchemaFieldSpec[] = []
  for (const [name, raw] of Object.entries(schema.properties)) {
    const skip = (why: string) => {
      if (import.meta.env.DEV) console.warn(`schema field "${name}" skipped: ${why}`)
    }
    if (!isObject(raw)) {
      skip('not an object')
      continue
    }
    const unknown = Object.keys(raw).find((k) => !KEYWORDS.has(k))
    if (unknown) {
      skip(`unknown keyword ${unknown}`)
      continue
    }
    const label = message(raw['x-postit-label'])
    const kind = kindOf(raw)
    if (!label || !kind) {
      skip('no label or an unsupported type')
      continue
    }
    const noDefault = raw['x-postit-no-default'] === true
    const dependsOn = isObject(raw['x-postit-depends-on'])
      ? { field: String(raw['x-postit-depends-on'].field), equals: raw['x-postit-depends-on'].equals }
      : undefined
    fields.push({
      name,
      label,
      help: message(raw['x-postit-help']),
      required: required.has(name) || noDefault,
      kind,
      default: raw.default,
      noDefault,
      secret: raw['x-postit-secret'] === true,
      consentText: message(raw['x-postit-consent-text']),
      dependsOn,
    })
  }
  return fields
}

/** Whether a field is shown: it has no condition, or the field it depends on has that value. */
export function isVisible(field: SchemaFieldSpec, values: Record<string, unknown>): boolean {
  return !field.dependsOn || values[field.dependsOn.field] === field.dependsOn.equals
}

/** The values a form starts with: each field's default, and nothing for a secret or a field that must be chosen. */
export function initialValues(fields: SchemaFieldSpec[]): Record<string, unknown> {
  const values: Record<string, unknown> = {}
  for (const f of fields) {
    if (f.noDefault || f.secret || f.default === undefined) continue
    values[f.name] = f.default
  }
  return values
}

/** What goes to the server: the fields that are shown, without empty optional text. */
export function submittedValues(fields: SchemaFieldSpec[], values: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const f of fields) {
    if (!isVisible(f, values)) continue
    const v = values[f.name]
    if (v === undefined) continue
    if (typeof v === 'string' && v === '' && !f.required) continue
    out[f.name] = v
  }
  return out
}
