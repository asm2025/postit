import { useTranslation } from 'react-i18next'
import { Eyebrow } from '@/components/kit/Eyebrow'
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
      <header className="flex flex-col gap-1.5">
        <Eyebrow className="text-faint">{dateLabel()}</Eyebrow>
        <h1 className="font-display text-[clamp(30px,5vw,40px)] leading-[1.05] font-bold tracking-[-0.02em]">
          {first ? t('greeting', { greeting: greeting(), name: isolate(first) }) : greeting()}
        </h1>
      </header>
      {me.data?.role === 'admin' && <AdminPanels />}
      <div className="flex flex-wrap gap-6">
        <div className="min-w-0 flex-[5_1_300px]">
          <AccountCard />
        </div>
      </div>
    </div>
  )
}
