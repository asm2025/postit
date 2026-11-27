import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { isolate } from '@/i18n/bidi'
import { useSaveUserLimits, useUserLimits, type UserLimits } from './queries'

/** The three limits an admin sets per user; `0` means unlimited. */
export function UserLimitsDialog({ userId, name, onClose }: { userId: string; name: string; onClose: () => void }) {
  const { t } = useTranslation(['admin', 'common'])
  const limits = useUserLimits(userId)
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('admin:limits.title')}</DialogTitle>
          <DialogDescription>{t('admin:limits.body', { name: isolate(name) })}</DialogDescription>
        </DialogHeader>
        {limits.isError && <ErrorText error={limits.error} />}
        {limits.data ? (
          <LimitsForm userId={userId} limits={limits.data} onClose={onClose} />
        ) : (
          !limits.isError && <p className="text-muted-foreground">{t('common:loading')}</p>
        )}
      </DialogContent>
    </Dialog>
  )
}

function LimitsForm({ userId, limits, onClose }: { userId: string; limits: UserLimits; onClose: () => void }) {
  const { t } = useTranslation(['admin', 'common'])
  const save = useSaveUserLimits(userId)
  const [contacts, setContacts] = useState(String(limits.contacts_cap))
  const [emails, setEmails] = useState(String(limits.email_daily_cap))
  const [storage, setStorage] = useState(String(limits.storage_quota_bytes))
  const parse = (text: string) => (/^\d+$/.test(text.trim()) ? Number(text.trim()) : null)
  const values = { contacts_cap: parse(contacts), email_daily_cap: parse(emails), storage_quota_bytes: parse(storage) }
  const valid = values.contacts_cap !== null && values.email_daily_cap !== null && values.storage_quota_bytes !== null

  const field = (label: string, value: string, set: (v: string) => void, help?: string) => (
    <div className="flex flex-col gap-1">
      <FormField label={label}>
        <Input
          type="number"
          inputMode="numeric"
          min={0}
          step={1}
          dir="ltr"
          value={value}
          onChange={(e) => {
            set(e.target.value)
          }}
        />
      </FormField>
      {help && <span className="text-xs text-muted-foreground">{help}</span>}
    </div>
  )
  return (
    <>
      {field(t('admin:limits.contacts'), contacts, setContacts, t('admin:limits.unlimited'))}
      {field(t('admin:limits.emails'), emails, setEmails, t('admin:limits.unlimited'))}
      {field(t('admin:limits.storage'), storage, setStorage, t('admin:limits.storageHelp'))}
      <ErrorText error={save.error ? messageFor(save.error) : null} />
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          {t('common:actions.cancel')}
        </Button>
        <Button
          disabled={!valid || save.isPending}
          onClick={() => {
            if (values.contacts_cap === null || values.email_daily_cap === null || values.storage_quota_bytes === null)
              return
            save.mutate(
              {
                contacts_cap: values.contacts_cap,
                email_daily_cap: values.email_daily_cap,
                storage_quota_bytes: values.storage_quota_bytes,
              },
              { onSuccess: onClose },
            )
          }}
        >
          {t('common:actions.save')}
        </Button>
      </DialogFooter>
    </>
  )
}
