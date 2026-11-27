import { useTranslation } from 'react-i18next'
import { Outlet } from 'react-router'
import { EmptyState } from '@/components/kit/EmptyState'
import { Button } from '@/components/ui/button'
import { useWorkspace } from './context'
import { useAccess } from './useAccess'

/** A route element for pages only the workspace owner can use. */
export function RequireOwn() {
  const { t } = useTranslation()
  const { switchTo } = useWorkspace()
  const access = useAccess()
  if (access.own) return <Outlet />
  return (
    <div className="flex flex-col items-start gap-3">
      <EmptyState>{t('workspace.ownerOnly.title')}</EmptyState>
      <Button
        variant="outline"
        onClick={() => {
          switchTo(null)
        }}
      >
        {t('workspace.ownerOnly.action')}
      </Button>
    </div>
  )
}
