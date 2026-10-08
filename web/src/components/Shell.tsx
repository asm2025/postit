import { Menu } from 'lucide-react'
import { useState } from 'react'
import { NavLink, Outlet } from 'react-router'
import { useSession } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import { useConfig } from '@/config/ConfigContext'
import { useMe } from '@/features/me/useMe'
import { usePendingCount } from '@/features/users/queries'
import { useTheme, type Theme } from '@/theme/ThemeProvider'

function Nav({ onNavigate }: { onNavigate?: () => void }) {
  const me = useMe()
  const isAdmin = me.data?.role === 'admin'
  const pending = usePendingCount(isAdmin)
  const link = ({ isActive }: { isActive: boolean }) =>
    `flex items-center justify-between rounded-md px-3 py-2 text-sm ${isActive ? 'bg-accent font-medium' : 'hover:bg-accent/60'}`
  return (
    <nav className="flex flex-col gap-1" onClick={onNavigate}>
      <NavLink to="/" end className={link}>
        Home
      </NavLink>
      <NavLink to="/profile" className={link}>
        Profile
      </NavLink>
      {isAdmin && (
        <>
          <NavLink to="/users" className={link}>
            Users
            {!!pending.data && <Badge>{pending.data}</Badge>}
          </NavLink>
          <NavLink to="/audit" className={link}>
            Audit
          </NavLink>
        </>
      )}
    </nav>
  )
}

function ThemeSelect() {
  const { theme, setTheme } = useTheme()
  return (
    <select
      aria-label="Theme"
      className="rounded-md border bg-background px-2 py-1 text-sm"
      value={theme}
      onChange={(e) => {
        setTheme(e.target.value as Theme)
      }}
    >
      <option value="system">System</option>
      <option value="light">Light</option>
      <option value="dark">Dark</option>
    </select>
  )
}

export function Shell() {
  const [open, setOpen] = useState(false)
  const { signOut } = useSession()
  const { runtime } = useConfig()
  return (
    <div className="min-h-screen md:grid md:grid-cols-[14rem_1fr]">
      <aside className="hidden border-r p-4 md:block">
        <div className="mb-4 text-lg font-semibold">postit</div>
        <Nav />
      </aside>
      <div className="flex min-w-0 flex-col">
        <header className="flex items-center gap-2 border-b p-3">
          <Button
            variant="ghost"
            size="icon"
            className="md:hidden"
            aria-label="Open menu"
            onClick={() => {
              setOpen(true)
            }}
          >
            <Menu />
          </Button>
          <div className="ml-auto flex items-center gap-2">
            {runtime.ENV !== 'production' && <Badge variant="secondary">{runtime.ENV}</Badge>}
            <ThemeSelect />
            <Button variant="outline" size="sm" onClick={signOut}>
              Sign out
            </Button>
          </div>
        </header>
        <main className="min-w-0 flex-1 p-4 md:p-6">
          <Outlet />
        </main>
      </div>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent side="left" className="w-56 p-4">
          <SheetTitle>postit</SheetTitle>
          <Nav
            onNavigate={() => {
              setOpen(false)
            }}
          />
        </SheetContent>
      </Sheet>
    </div>
  )
}
