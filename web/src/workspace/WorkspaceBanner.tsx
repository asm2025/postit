import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useWorkspace } from './context'

/** Under the header while the tab works in someone else's workspace, and why it left one. */
export function WorkspaceBanner() {
  const { t } = useTranslation()
  const { current, switchTo, endedNotice, clearEndedNotice } = useWorkspace()
  const delegated = current && !current.own
  if (!delegated && !endedNotice) return null
  return (
    <div className="flex flex-col gap-2 px-5 pt-3 md:px-14">
      {endedNotice && (
        <div role="alert" className="flex items-center gap-2 rounded-xl border border-line bg-raised px-4 py-2 text-sm">
          <span className="flex-1">{endedNotice}</span>
          <button
            type="button"
            aria-label={t('workspace.dismiss')}
            onClick={clearEndedNotice}
            className="flex size-8 cursor-pointer items-center justify-center rounded-[10px] border border-transparent bg-transparent text-muted-foreground hover:border-faint"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>
      )}
      {delegated && (
        <div
          role="status"
          className="flex flex-wrap items-center gap-3 rounded-xl border border-line bg-raised px-4 py-2 text-sm"
        >
          <span className="flex-1">
            {t('workspace.banner', {
              name: current.owner.display_name,
              level: t(`workspace.level.${current.level ?? 'view'}`),
            })}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              switchTo(null)
            }}
          >
            {t('workspace.back')}
          </Button>
        </div>
      )}
    </div>
  )
}
