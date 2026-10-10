import { NavLink } from 'react-router'
import { cn } from 'cn'

const LINKS = [
  { to: '/jobs', label: 'Dashboard', end: true },
  { to: '/jobs/list', label: 'Jobs', end: false },
  { to: '/jobs/recurring', label: 'Recurring', end: false },
]

/** Sub-navigation shared by the job console pages. */
export function JobsNav() {
  return (
    <nav aria-label="Jobs sections" className="flex flex-wrap gap-1.5">
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
          {l.label}
        </NavLink>
      ))}
    </nav>
  )
}
