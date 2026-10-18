import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'
import { cn } from 'cn'

const LINKS = [
  { to: '/jobs', label: 'dashboard', end: true },
  { to: '/jobs/list', label: 'jobs', end: false },
  { to: '/jobs/recurring', label: 'recurring', end: false },
] as const

/** Sub-navigation shared by the job console pages. */
export function JobsNav() {
  const { t } = useTranslation('jobs')
  return (
    <nav aria-label={t('nav.label')} className="flex flex-wrap gap-1.5">
      {LINKS.map((l) => (
        <NavLink
          key={l.to}
          to={l.to}
          end={l.end}
          className={({ isActive }) =>
            cn(
              'inline-flex min-h-11 items-center rounded-lg border px-3.5 text-sm font-medium',
              isActive ? 'border-pink bg-pink-soft text-ink' : 'border-line bg-surface text-ink hover:bg-raised',
            )
          }
        >
          {t(`nav.${l.label}`)}
        </NavLink>
      ))}
    </nav>
  )
}
