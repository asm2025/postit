import { FileText, Home, ListChecks, LogOut, Menu, User, Users, type LucideIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, Outlet } from 'react-router'
import { useSession } from '@/auth/session'
import { Eyebrow } from '@/components/kit/Eyebrow'
import { Logo } from '@/components/Logo'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import { useConfig } from '@/config/ConfigContext'
import { useMe } from '@/features/me/useMe'
import { useFailedJobCount } from '@/features/jobs/queries'
import { usePendingCount } from '@/features/users/queries'
import { useFormat } from '@/i18n/useFormat'
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
  const { t } = useTranslation()
  const { number } = useFormat()
  const me = useMe()
  const isAdmin = me.data?.role === 'admin'
  const pending = usePendingCount(isAdmin)
  const failed = useFailedJobCount(isAdmin)
  return (
    <nav aria-label={t('shell.mainNav')} className="flex flex-col gap-[22px]" onClick={onNavigate}>
      <div className="flex flex-col gap-0.5">
        <NavItem to="/" end Icon={Home}>
          {t('shell.home')}
        </NavItem>
        <NavItem to="/profile" Icon={User}>
          {t('shell.profile')}
        </NavItem>
      </div>
      {isAdmin && (
        <div className="flex flex-col gap-0.5">
          <Eyebrow className="px-3 pb-1.5 text-[11px] text-faint rtl:text-sm">{t('shell.admin')}</Eyebrow>
          <NavItem to="/users" Icon={Users}>
            <span className="flex-1">{t('shell.users')}</span>
            {!!pending.data && <span className="font-mono text-xs text-faint">{number(pending.data)}</span>}
          </NavItem>
          <NavItem to="/audit" Icon={FileText}>
            {t('shell.audit')}
          </NavItem>
          <NavItem to="/jobs" Icon={ListChecks}>
            <span className="flex-1">{t('shell.jobs')}</span>
            {!!failed.data && (
              <span className="font-mono text-xs text-faint">
                {number(failed.data)}
                <span className="sr-only"> {t('shell.failedJobs')}</span>
              </span>
            )}
          </NavItem>
        </div>
      )}
    </nav>
  )
}

function SidebarFooter() {
  const { t } = useTranslation()
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
          <div dir="auto" className="truncate text-sm font-semibold">
            {name}
          </div>
          <div dir="ltr" className="truncate text-xs text-muted-foreground rtl:text-end">
            {me.data?.email ?? ''}
          </div>
        </div>
        <button
          type="button"
          aria-label={t('actions.signOut')}
          onClick={signOut}
          className="flex size-11 flex-none cursor-pointer items-center justify-center rounded-[10px] border border-transparent bg-transparent text-muted-foreground hover:border-faint"
        >
          <LogOut className="size-[18px] rtl:-scale-x-100" aria-hidden="true" />
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
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <div className="min-h-screen md:grid md:grid-cols-[15.5rem_1fr]">
      <aside className="sticky top-0 hidden h-screen border-e border-line p-4 py-6 md:block">
        <Sidebar />
      </aside>
      <div className="flex min-w-0 flex-col">
        <header className="flex items-center gap-2 px-5 pt-4 md:justify-end md:px-14">
          <button
            type="button"
            aria-label={t('shell.openMenu')}
            className="flex size-11 cursor-pointer items-center justify-center rounded-[10px] border border-line bg-surface md:hidden"
            onClick={() => {
              setOpen(true)
            }}
          >
            <Menu className="size-5" aria-hidden="true" />
          </button>
          <div className="ms-auto md:ms-0">
            <ThemeToggle />
          </div>
        </header>
        <main className="flex min-w-0 flex-1 flex-col px-5 pt-6 pb-14 md:px-14">
          {/* Fills the height, so a page can pin a section (the danger zone) to the bottom. */}
          <div className="mx-auto flex w-full max-w-[1120px] flex-1 flex-col">
            <Outlet />
          </div>
        </main>
      </div>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent side="inline-start" className="w-72 p-4 pt-6">
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
