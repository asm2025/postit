import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

export interface Option {
  value: string
  label: string
  /** For values that are code, not words (a job type): kept left-to-right. */
  dir?: 'ltr'
}

interface Props {
  value: string
  options: readonly Option[]
  onChange: (value: string) => void
  /** Shown first and selected for an empty value: "Any status". */
  anyLabel?: string
  'aria-label'?: string
}

// Radix Select reserves the empty string, so "any" travels as this and leaves as ''.
const ANY = '__any__'

/** A filter or setting picked from a short list, with an optional "any" entry. */
export function OptionSelect({ value, options, onChange, anyLabel, 'aria-label': ariaLabel }: Props) {
  return (
    <Select
      value={value || (anyLabel ? ANY : undefined)}
      onValueChange={(v) => {
        onChange(v === ANY ? '' : v)
      }}
    >
      <SelectTrigger className="min-w-40" aria-label={ariaLabel}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {anyLabel && <SelectItem value={ANY}>{anyLabel}</SelectItem>}
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value} dir={o.dir}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
