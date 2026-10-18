import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApi } from '@/api/ApiContext'
import { ensureOk } from '@/api/client'
import { useSession } from '@/auth/session'
import { DangerZone } from '@/components/kit/DangerZone'
import { ErrorText } from '@/components/kit/ErrorText'
import { FactList } from '@/components/kit/FactList'
import { PageHeader } from '@/components/kit/PageHeader'
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
import { Label } from '@/components/ui/label'
import { isolate } from '@/i18n/bidi'
import { LanguageSwitch } from '@/i18n/LanguageSwitch'
import { useMe } from './useMe'

export function ProfilePage() {
  const { t } = useTranslation(['me', 'common'])
  const me = useMe()
  const api = useApi()
  const { signOut } = useSession()
  const [open, setOpen] = useState(false)
  const [typed, setTyped] = useState('')
  const del = useMutation({
    mutationFn: (display_name: string) => ensureOk(api.DELETE('/api/v1/me', { body: { display_name } })),
    onSuccess: signOut,
  })
  if (!me.data) return <p className="text-muted-foreground">{t('common:loading')}</p>
  const name = me.data.display_name
  return (
    <div className="max-w-lg space-y-6">
      <PageHeader eyebrow={t('me:profile.eyebrow')} title={t('me:profile.title')} />
      {/* Values are isolated inline, so a Latin name or email keeps its direction but sits at the start. */}
      <FactList
        facts={[
          [t('me:profile.name'), <bdi dir="auto">{name}</bdi>],
          [t('me:profile.email'), <bdi dir="ltr">{me.data.email ?? '—'}</bdi>],
          [t('me:profile.role'), t(`common:role.${me.data.role}`)],
          [t('me:profile.language'), <LanguageSwitch persist />],
        ]}
      />
      <p className="text-sm text-muted-foreground">
        {t('me:profile.idpNote')}{' '}
        {me.data.account_url && (
          <a className="underline" href={me.data.account_url} target="_blank" rel="noreferrer">
            {t('common:actions.manageAccount')}
          </a>
        )}
      </p>
      <Button variant="outline" onClick={signOut}>
        {t('common:actions.signOut')}
      </Button>
      <DangerZone description={t('me:profile.dangerBody')}>
        <Button
          variant="destructive"
          onClick={() => {
            setOpen(true)
          }}
        >
          {t('me:profile.deleteButton')}
        </Button>
      </DangerZone>
      <Dialog
        open={open}
        onOpenChange={(o) => {
          setOpen(o)
          if (!o) {
            setTyped('')
            del.reset()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('me:profile.deleteTitle')}</DialogTitle>
            <DialogDescription>{t('me:profile.deleteBody')}</DialogDescription>
          </DialogHeader>
          <Label htmlFor="confirm-name">{t('me:profile.typeName', { name: isolate(name) })}</Label>
          <Input
            id="confirm-name"
            dir="auto"
            value={typed}
            onChange={(e) => {
              setTyped(e.target.value)
            }}
          />
          <ErrorText error={del.error} />
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setOpen(false)
              }}
            >
              {t('common:actions.cancel')}
            </Button>
            <Button
              variant="destructive"
              disabled={typed !== name || del.isPending}
              onClick={() => {
                del.mutate(name)
              }}
            >
              {t('me:profile.deleteConfirm')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
