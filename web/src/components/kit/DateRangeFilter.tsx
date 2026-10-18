import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/input'
import { FormField } from './FormField'

interface Props {
  /** `datetime-local` values (local time, no zone); empty for an open end. */
  from: string
  to: string
  onFromChange: (value: string) => void
  onToChange: (value: string) => void
}

/** A from/to pair of local date-times for filtering a list. */
export function DateRangeFilter({ from, to, onFromChange, onToChange }: Props) {
  const { t } = useTranslation()
  return (
    <>
      <FormField label={t('filters.from')}>
        <Input
          type="datetime-local"
          value={from}
          onChange={(e) => {
            onFromChange(e.target.value)
          }}
        />
      </FormField>
      <FormField label={t('filters.to')}>
        <Input
          type="datetime-local"
          value={to}
          onChange={(e) => {
            onToChange(e.target.value)
          }}
        />
      </FormField>
    </>
  )
}
