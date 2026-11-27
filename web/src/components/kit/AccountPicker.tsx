import { useId } from 'react'
import { Checkbox } from '@/components/ui/checkbox'
import { usePlatforms } from '@/features/platforms/queries'
import type { components } from '@/api/schema'
import { PlatformBadge } from './PlatformBadge'

type Account = components['schemas']['AccountDto']

interface Props {
  /** The chosen account IDs. */
  value: string[]
  onChange: (ids: string[]) => void
  /** Callers pass only the accounts the caller reaches. */
  accounts: Account[]
}

/** Accounts as checkboxes, grouped by platform. */
export function AccountPicker({ value, onChange, accounts }: Props) {
  const platforms = usePlatforms()
  const prefix = useId()
  const groups = new Map<string, Account[]>()
  for (const account of accounts) groups.set(account.platform, [...(groups.get(account.platform) ?? []), account])

  const toggle = (id: string, on: boolean) => {
    onChange(on ? [...value, id] : value.filter((v) => v !== id))
  }
  return (
    <div className="flex flex-col gap-4">
      {[...groups.entries()].map(([platformId, rows]) => {
        const platform = platforms.data?.find((p) => p.platform === platformId)
        const headingId = `${prefix}-${platformId}`
        return (
          <div key={platformId} role="group" aria-labelledby={headingId} className="flex flex-col gap-1.5">
            <div id={headingId} className="text-sm font-semibold">
              {platform ? <PlatformBadge platform={platform} size="sm" /> : <span dir="auto">{platformId}</span>}
            </div>
            {rows.map((account) => {
              const id = `${prefix}-${account.id}`
              return (
                <div key={account.id} className="flex items-center gap-2.5 ps-1">
                  <Checkbox
                    id={id}
                    checked={value.includes(account.id)}
                    onCheckedChange={(checked) => {
                      toggle(account.id, checked === true)
                    }}
                  />
                  <label htmlFor={id} dir="auto" className="text-sm">
                    {account.display_name}
                  </label>
                </div>
              )
            })}
          </div>
        )
      })}
    </div>
  )
}
