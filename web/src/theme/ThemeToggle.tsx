import { Monitor, Moon, Sun, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useTheme, type Theme } from './ThemeProvider'

const OPTIONS: { value: Theme; label: 'theme.system' | 'theme.light' | 'theme.dark'; Icon: LucideIcon }[] = [
  { value: 'system', label: 'theme.system', Icon: Monitor },
  { value: 'light', label: 'theme.light', Icon: Sun },
  { value: 'dark', label: 'theme.dark', Icon: Moon },
]

/** System / light / dark segmented control. */
export function ThemeToggle() {
  const { t } = useTranslation()
  const { theme, setTheme } = useTheme()
  return (
    <div
      role="group"
      aria-label={t('theme.group')}
      className="flex gap-0.5 rounded-xl border border-line bg-surface p-[3px]"
    >
      {OPTIONS.map(({ value, label, Icon }) => (
        <button
          key={value}
          type="button"
          aria-label={t(label)}
          aria-pressed={theme === value}
          onClick={() => {
            setTheme(value)
          }}
          className={`flex size-11 cursor-pointer items-center justify-center rounded-[9px] border-0 sm:size-10 ${
            theme === value ? 'bg-raised text-foreground' : 'bg-transparent text-faint hover:text-foreground'
          }`}
        >
          <Icon className="size-4" aria-hidden="true" />
        </button>
      ))}
    </div>
  )
}
