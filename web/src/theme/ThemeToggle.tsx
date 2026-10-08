import { Monitor, Moon, Sun, type LucideIcon } from 'lucide-react'
import { useTheme, type Theme } from './ThemeProvider'

const OPTIONS: { value: Theme; label: string; Icon: LucideIcon }[] = [
  { value: 'system', label: 'System theme', Icon: Monitor },
  { value: 'light', label: 'Light theme', Icon: Sun },
  { value: 'dark', label: 'Dark theme', Icon: Moon },
]

/** System / light / dark segmented control. */
export function ThemeToggle() {
  const { theme, setTheme } = useTheme()
  return (
    <div role="group" aria-label="Theme" className="flex gap-0.5 rounded-xl border border-line bg-surface p-[3px]">
      {OPTIONS.map(({ value, label, Icon }) => (
        <button
          key={value}
          type="button"
          aria-label={label}
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
