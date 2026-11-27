import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { AccountPicker } from '@/components/kit/AccountPicker'
import { ErrorText } from '@/components/kit/ErrorText'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAccounts, useSaveTargetGroup, type TargetGroup } from './queries'

/**
 * Creates a group or edits one. The picker offers the accounts the caller reaches; saving replaces
 * the visible members and the server keeps the ones the caller cannot see.
 */
export function TargetGroupDialog({ group, onClose }: { group?: TargetGroup; onClose: () => void }) {
  const { t } = useTranslation(['accounts', 'common'])
  const accounts = useAccounts({ page: 1, pageSize: 100 })
  const save = useSaveTargetGroup()
  const [name, setName] = useState(group?.name ?? '')
  const [chosen, setChosen] = useState(group?.account_ids ?? [])

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {group ? t('accounts:groupsPage.dialog.editTitle') : t('accounts:groupsPage.dialog.createTitle')}
          </DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="group-name">{t('accounts:groupsPage.dialog.name')}</Label>
          <Input
            id="group-name"
            dir="auto"
            maxLength={100}
            value={name}
            onChange={(e) => {
              setName(e.target.value)
            }}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label>{t('accounts:groupsPage.dialog.accounts')}</Label>
          {accounts.data?.data.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('accounts:groupsPage.dialog.none')}</p>
          ) : accounts.data ? (
            <div className="max-h-64 overflow-y-auto">
              <AccountPicker value={chosen} onChange={setChosen} accounts={accounts.data.data} />
            </div>
          ) : null}
        </div>
        <ErrorText error={save.error ? messageFor(save.error) : null} />
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common:actions.cancel')}
          </Button>
          <Button
            disabled={save.isPending || name.trim() === ''}
            onClick={() => {
              save.mutate({ id: group?.id, body: { name: name.trim(), account_ids: chosen } }, { onSuccess: onClose })
            }}
          >
            {t('accounts:groupsPage.dialog.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
