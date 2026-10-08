import { useMe } from '@/features/me/useMe'
import { dateLabel, greeting } from '@/lib/format'
import { AccountCard } from './AccountCard'
import { AdminPanels } from './AdminPanels'

export function HomePage() {
  const me = useMe()
  const first = me.data?.display_name.split(/\s+/)[0]
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1.5">
        <div className="font-mono text-xs tracking-[0.06em] text-faint uppercase">{dateLabel()}</div>
        <h1 className="font-display text-[clamp(30px,5vw,40px)] leading-[1.05] font-bold tracking-[-0.02em]">
          {greeting()}
          {first ? `, ${first}` : ''}
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
