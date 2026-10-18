import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApi } from '@/api/ApiContext'
import { ensureOk } from '@/api/client'
import { messageFor } from '@/api/errors'
import { useSession } from '@/auth/session'
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
      <div className="space-y-1.5">
        <div className="font-mono text-xs tracking-[0.06em] text-faint uppercase">{t('me:profile.eyebrow')}</div>
        <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">{t('me:profile.title')}</h1>
      </div>
      <dl className="grid grid-cols-[8rem_1fr] gap-2 text-sm">
        <dt className="text-muted-foreground">{t('me:profile.name')}</dt>
        {/* Isolated inline, so a Latin value keeps its own direction but sits at the start of the cell. */}
        <dd>
          <bdi dir="auto">{name}</bdi>
        </dd>
        <dt className="text-muted-foreground">{t('me:profile.email')}</dt>
        <dd>
          <bdi dir="ltr">{me.data.email ?? '—'}</bdi>
        </dd>
        <dt className="text-muted-foreground">{t('me:profile.role')}</dt>
        <dd>{t(`common:role.${me.data.role}`)}</dd>
        <dt className="text-muted-foreground">{t('me:profile.language')}</dt>
        <dd>
          <LanguageSwitch persist />
        </dd>
      </dl>
      <p className="text-sm text-muted-foreground">
        {t('me:profile.idpNote')}{' '}
        {me.data.account_url && (
          <a className="underline" href={me.data.account_url} target="_blank" rel="noreferrer">
            {t('common:actions.manageAccount')}
          </a>
        )}
      </p>
      <div className="flex gap-2">
        <Button variant="outline" onClick={signOut}>
          {t('common:actions.signOut')}
        </Button>
        <Button
          variant="destructive"
          onClick={() => {
            setOpen(true)
          }}
        >
          {t('me:profile.deleteButton')}
        </Button>
      </div>
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
          {del.error && (
            <p role="alert" className="text-sm text-destructive">
              {messageFor(del.error)}
            </p>
          )}
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
