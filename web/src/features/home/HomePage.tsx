import { useTranslation } from 'react-i18next'
import { PageHeader } from '@/components/kit/PageHeader'
import { useMe } from '@/features/me/useMe'
import { isolate } from '@/i18n/bidi'
import { useFormat } from '@/i18n/useFormat'
import { AccountCard } from './AccountCard'
import { AdminPanels } from './AdminPanels'

export function HomePage() {
  const { t } = useTranslation('home')
  const { dateLabel, greeting } = useFormat()
  const me = useMe()
  const first = me.data?.display_name.split(/\s+/)[0]
  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={dateLabel()}
        title={first ? t('greeting', { greeting: greeting(), name: isolate(first) }) : greeting()}
      />
      {me.data?.role === 'admin' && <AdminPanels />}
      <div className="flex flex-wrap gap-6">
        <div className="min-w-0 flex-[5_1_300px]">
          <AccountCard />
        </div>
      </div>
    </div>
  )
}
