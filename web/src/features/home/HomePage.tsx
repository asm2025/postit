import { useMe } from '@/features/me/useMe'
import { AccountCard } from './AccountCard'
import { AdminPanels } from './AdminPanels'

export function HomePage() {
  const me = useMe()
  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Home</h1>
      {me.data?.role === 'admin' && <AdminPanels />}
      <AccountCard />
    </div>
  )
}
