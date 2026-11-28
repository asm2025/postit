import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

export interface Option {
  value: string
  label: string
  /** For values that are code, not words (a job type): kept left-to-right. */
  dir?: 'ltr'
  /** With `fill`, the shorter text the trigger shows when this option is selected. */
  short?: string
}

interface Props {
  value: string
  options: readonly Option[]
  onChange: (value: string) => void
  /** Shown first and selected for an empty value: "Any status". */
  anyLabel?: string
  'aria-label'?: string
  disabled?: boolean
  /**
   * For long labels in a narrow column (the sidebar's workspace switcher): the trigger fills its
   * container and shows the selected option's `short` text (or its label) on one line, cut off
   * with an ellipsis, with the full label as its tooltip; the open list wraps long labels.
   */
  fill?: boolean
}

// Radix Select reserves the empty string, so "any" travels as this and leaves as ''.
const ANY = '__any__'

// Overrides the trigger's `w-fit` and lets the one-line value shrink so it ends in an ellipsis.
const FILL_TRIGGER =
  'w-full min-w-0 text-start *:data-[slot=select-value]:block *:data-[slot=select-value]:truncate *:data-[slot=select-value]:min-w-0 *:data-[slot=select-value]:flex-1'
const FILL_CONTENT = 'max-w-72'
const FILL_ITEM = 'whitespace-normal break-words'

/** A filter or setting picked from a short list, with an optional "any" entry. */
export function OptionSelect({
  value,
  options,
  onChange,
  anyLabel,
  'aria-label': ariaLabel,
  disabled,
  fill = false,
}: Props) {
  const selected = fill ? options.find((o) => o.value === value) : undefined
  const title = fill ? (selected?.label ?? (value ? undefined : anyLabel)) : undefined
  return (
    <Select
      value={value || (anyLabel ? ANY : undefined)}
      disabled={disabled}
      onValueChange={(v) => {
        onChange(v === ANY ? '' : v)
      }}
    >
      <SelectTrigger className={fill ? FILL_TRIGGER : 'min-w-40'} aria-label={ariaLabel} title={title}>
        <SelectValue>{selected ? (selected.short ?? selected.label) : undefined}</SelectValue>
      </SelectTrigger>
      <SelectContent className={fill ? FILL_CONTENT : undefined}>
        {anyLabel && (
          <SelectItem value={ANY} className={fill ? FILL_ITEM : undefined}>
            {anyLabel}
          </SelectItem>
        )}
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value} dir={o.dir} className={fill ? FILL_ITEM : undefined}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
