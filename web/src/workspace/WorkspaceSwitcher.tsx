import { useTranslation } from 'react-i18next'
import { OptionSelect, type Option } from '@/components/kit/OptionSelect'
import { useWorkspace, type WorkspaceDto } from './context'

const OWN = 'own'

/** The workspace the tab can act in: the user's own first, then each one a live grant opens. */
export function WorkspaceSwitcher() {
  const { t } = useTranslation()
  const { workspaces, actAs, switchTo } = useWorkspace()
  // Nothing to choose between.
  if (workspaces.length < 2) return null

  const label = (w: WorkspaceDto): string => {
    if (w.own) return t('workspace.own')
    const parts = [t('workspace.of', { name: w.owner.display_name })]
    if (w.level) parts.push(t(`workspace.level.${w.level}`))
    if (w.all_accounts === false) parts.push(t('workspace.accounts', { count: w.account_ids?.length ?? 0 }))
    return parts.join(' · ')
  }
  const options: Option[] = workspaces.map((w) => ({ value: w.own ? OWN : w.owner.id, label: label(w) }))
  return (
    <div className="px-1">
      <OptionSelect
        aria-label={t('workspace.switcher')}
        value={actAs ?? OWN}
        options={options}
        onChange={(value) => {
          switchTo(value === OWN || value === '' ? null : value)
        }}
      />
    </div>
  )
}
