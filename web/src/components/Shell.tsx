import { FileText, Home, LogOut, Menu, User, Users, type LucideIcon } from 'lucide-react'
import { useState } from 'react'
import { NavLink, Outlet } from 'react-router'
import { useSession } from '@/auth/session'
import { Logo } from '@/components/Logo'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import { useConfig } from '@/config/ConfigContext'
import { useMe } from '@/features/me/useMe'
import { usePendingCount } from '@/features/users/queries'
import { initials } from '@/lib/format'
import { ThemeToggle } from '@/theme/ThemeToggle'

function NavItem({
  to,
  end,
  Icon,
  children,
}: {
  to: string
  end?: boolean
  Icon: LucideIcon
  children: React.ReactNode
}) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) =>
        `flex min-h-11 items-center gap-3 rounded-[10px] px-3 no-underline ${
          isActive ? 'bg-raised font-semibold !text-foreground' : 'font-medium !text-muted-foreground hover:bg-raised'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <Icon className={`size-[18px] ${isActive ? 'text-pink' : ''}`} aria-hidden="true" />
          {children}
        </>
      )}
    </NavLink>
  )
}

function Nav({ onNavigate }: { onNavigate?: () => void }) {
  const me = useMe()
  const isAdmin = me.data?.role === 'admin'
  const pending = usePendingCount(isAdmin)
  return (
    <nav aria-label="Main" className="flex flex-col gap-[22px]" onClick={onNavigate}>
      <div className="flex flex-col gap-0.5">
        <NavItem to="/" end Icon={Home}>
          Home
        </NavItem>
        <NavItem to="/profile" Icon={User}>
          Profile
        </NavItem>
      </div>
      {isAdmin && (
        <div className="flex flex-col gap-0.5">
          <div className="px-3 pb-1.5 font-mono text-[11px] tracking-[0.08em] text-faint uppercase">Admin</div>
          <NavItem to="/users" Icon={Users}>
            <span className="flex-1">Users</span>
            {!!pending.data && <span className="font-mono text-xs text-faint">{pending.data}</span>}
          </NavItem>
          <NavItem to="/audit" Icon={FileText}>
            Audit
          </NavItem>
        </div>
      )}
    </nav>
  )
}

function SidebarFooter() {
  const me = useMe()
  const { signOut } = useSession()
  const { runtime } = useConfig()
  const name = me.data?.display_name ?? ''
  return (
    <div className="mt-auto flex flex-col gap-3">
      <div className="flex items-center gap-2 px-3 font-mono text-xs text-muted-foreground">
        <span className="size-[7px] rounded-full bg-warn-icon" aria-hidden="true" />
        {runtime.ENV}
      </div>
      <div className="flex items-center gap-2.5 rounded-xl border border-line bg-surface p-2.5">
        <div
          aria-hidden="true"
          className="flex size-9 flex-none items-center justify-center rounded-[10px] bg-pink font-display text-sm font-bold text-on-pink"
        >
          {initials(name)}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold">{name}</div>
          <div className="truncate text-xs text-muted-foreground">{me.data?.email ?? ''}</div>
        </div>
        <button
          type="button"
          aria-label="Sign out"
          onClick={signOut}
          className="flex size-11 flex-none cursor-pointer items-center justify-center rounded-[10px] border border-transparent bg-transparent text-muted-foreground hover:border-faint"
        >
          <LogOut className="size-[18px]" aria-hidden="true" />
        </button>
      </div>
    </div>
  )
}

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <div className="flex h-full flex-col gap-7">
      <div className="px-3 py-1 text-ink">
        <Logo className="h-[37px] w-[92px]" />
      </div>
      <Nav onNavigate={onNavigate} />
      <SidebarFooter />
    </div>
  )
}

export function Shell() {
  const [open, setOpen] = useState(false)
  return (
    <div className="min-h-screen md:grid md:grid-cols-[15.5rem_1fr]">
      <aside className="sticky top-0 hidden h-screen border-r border-line p-4 py-6 md:block">
        <Sidebar />
      </aside>
      <div className="flex min-w-0 flex-col">
        <header className="flex items-center gap-2 px-5 pt-4 md:justify-end md:px-14">
          <button
            type="button"
            aria-label="Open menu"
            className="flex size-11 cursor-pointer items-center justify-center rounded-[10px] border border-line bg-surface md:hidden"
            onClick={() => {
              setOpen(true)
            }}
          >
            <Menu className="size-5" aria-hidden="true" />
          </button>
          <div className="ml-auto md:ml-0">
            <ThemeToggle />
          </div>
        </header>
        <main className="min-w-0 flex-1 px-5 pt-6 pb-14 md:px-14">
          <div className="mx-auto max-w-[1120px]">
            <Outlet />
          </div>
        </main>
      </div>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent side="left" className="w-72 p-4 pt-6">
          <SheetTitle className="sr-only">postit</SheetTitle>
          <Sidebar
            onNavigate={() => {
              setOpen(false)
            }}
          />
        </SheetContent>
      </Sheet>
    </div>
  )
}
