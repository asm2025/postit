import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'
import { pillClass } from '@/components/kit/pill'

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
        <NavLink key={l.to} to={l.to} end={l.end} className={({ isActive }) => pillClass(isActive)}>
          {t(`nav.${l.label}`)}
        </NavLink>
      ))}
    </nav>
  )
}
