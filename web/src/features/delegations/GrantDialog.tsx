import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, messageFor } from '@/api/errors'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { AccountPicker } from '@/components/kit/AccountPicker'
import { DangerZone } from '@/components/kit/DangerZone'
import { ErrorText } from '@/components/kit/ErrorText'
import { fromLocalInput, toLocalInput } from '@/components/kit/datetime'
import { OptionSelect } from '@/components/kit/OptionSelect'
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
import { useAccounts } from '@/features/accounts/queries'
import { isolate } from '@/i18n/bidi'
import {
  useChangeGrant,
  useGrant,
  useRevoke,
  useUserLookup,
  type ChangeRequest,
  type Delegation,
  type GrantRequest,
} from './queries'

type Level = Delegation['level']
const LEVELS: Level[] = ['view', 'edit', 'publish']
const LOOKUP_DELAY_MS = 400

/** The end of the chosen day, in the browser's zone, as RFC 3339. */
const endOfDay = (date: string) => fromLocalInput(`${date}T23:59`)
const dateOf = (iso: string | null | undefined) => (iso ? toLocalInput(iso).slice(0, 10) : '')

/** Grants access to a user found by exact email, or changes a grant; revoking sits at the bottom of the edit form. */
export function GrantDialog({ open, onClose, grant }: { open: boolean; onClose: () => void; grant?: Delegation }) {
  const { t } = useTranslation(['delegations', 'common'])
  const editing = grant !== undefined
  const [email, setEmail] = useState('')
  const [typed, setTyped] = useState('')
  const [level, setLevel] = useState<Level>(grant?.level ?? 'view')
  const [all, setAll] = useState(grant?.all_accounts ?? true)
  const [chosen, setChosen] = useState<string[]>(grant?.account_ids ?? [])
  const [expiry, setExpiry] = useState(dateOf(grant?.expires_at))
  const [confirmRevoke, setConfirmRevoke] = useState(false)

  // Look up once the user stops typing; the server answers exact matches only.
  useEffect(() => {
    const timer = setTimeout(() => {
      setEmail(typed.trim())
    }, LOOKUP_DELAY_MS)
    return () => {
      clearTimeout(timer)
    }
  }, [typed])
  const lookup = useUserLookup(editing ? '' : email)
  const accounts = useAccounts({ page: 1, pageSize: 100 })
  const create = useGrant()
  const change = useChangeGrant()
  const revoke = useRevoke()

  const found = lookup.data
  const waiting = lookup.error instanceof ApiError && lookup.error.code === 'rate_limited' ? lookup.error : null
  const unchanged = grant && expiry === dateOf(grant.expires_at)
  const busy = create.isPending || change.isPending || revoke.isPending
  const canSubmit = !busy && (editing || !!found) && (all || accounts.data !== undefined)
  const error = create.error ?? change.error ?? revoke.error

  const submit = () => {
    const limit = all ? {} : { account_ids: chosen }
    if (grant) {
      const body: ChangeRequest = {
        level,
        ...(all ? { all_accounts: true } : limit),
        // Every field goes, the expiry included: an absent one would be left alone, and an unchanged one must not be cleared.
        expires_at: unchanged ? (grant.expires_at ?? null) : expiry ? endOfDay(expiry) : null,
      }
      change.mutate({ id: grant.id, body }, { onSuccess: onClose })
    } else if (found) {
      const body: GrantRequest = {
        delegate_id: found.id,
        level,
        ...limit,
        expires_at: expiry ? endOfDay(expiry) : null,
      }
      create.mutate(body, { onSuccess: onClose })
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{editing ? t('delegations:grant.editTitle') : t('delegations:grant.title')}</DialogTitle>
          <DialogDescription>
            {editing
              ? t('delegations:grant.editBody', { name: isolate(grant.delegate.display_name) })
              : t('delegations:grant.body')}
          </DialogDescription>
        </DialogHeader>

        {!editing && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="grant-email">{t('delegations:grant.email')}</Label>
            <Input
              id="grant-email"
              type="email"
              dir="ltr"
              autoComplete="off"
              value={typed}
              onChange={(e) => {
                setTyped(e.target.value)
              }}
            />
            {email !== '' && found && (
              <p className="text-sm text-muted-foreground">
                {t('delegations:grant.found', { name: isolate(found.display_name) })}
              </p>
            )}
            {email !== '' && found === null && (
              <p role="alert" className="text-sm text-destructive">
                {t('delegations:grant.notFound')}
              </p>
            )}
            {waiting && (
              <p role="alert" className="text-sm text-destructive">
                {t('delegations:grant.wait', { seconds: waiting.retryAfter ?? 60 })}
              </p>
            )}
            {lookup.isError && !waiting && <ErrorText error={lookup.error} />}
          </div>
        )}

        <div className="flex flex-col gap-1.5">
          <Label>{t('delegations:grant.level')}</Label>
          <OptionSelect
            aria-label={t('delegations:grant.level')}
            value={level}
            options={LEVELS.map((l) => ({ value: l, label: t(`delegations:level.${l}`) }))}
            onChange={(v) => {
              setLevel(v as Level)
            }}
          />
          <p className="text-sm text-muted-foreground">{t(`delegations:levelHelp.${level}`)}</p>
        </div>

        <fieldset className="flex flex-col gap-2">
          <legend className="text-sm font-medium">{t('delegations:grant.accounts')}</legend>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="radio"
              name="scope"
              checked={all}
              onChange={() => {
                setAll(true)
              }}
            />
            {t('delegations:grant.allAccounts')}
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="radio"
              name="scope"
              checked={!all}
              onChange={() => {
                setAll(false)
              }}
            />
            {t('delegations:grant.chosenAccounts')}
          </label>
          {!all && accounts.data && (
            <div className="max-h-48 overflow-y-auto ps-6">
              <AccountPicker value={chosen} onChange={setChosen} accounts={accounts.data.data} />
            </div>
          )}
        </fieldset>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="grant-expiry">{t('delegations:grant.expiry')}</Label>
          <Input
            id="grant-expiry"
            type="date"
            value={expiry}
            onChange={(e) => {
              setExpiry(e.target.value)
            }}
          />
          <p className="text-sm text-muted-foreground">{t('delegations:grant.expiryHelp')}</p>
        </div>

        <ErrorText error={error ? messageFor(error) : null} />
        {editing && (
          <DangerZone description={t('delegations:revoke.body')}>
            <Button
              variant="destructive"
              onClick={() => {
                setConfirmRevoke(true)
              }}
            >
              {t('delegations:revoke.button')}
            </Button>
          </DangerZone>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common:actions.cancel')}
          </Button>
          <Button disabled={!canSubmit} onClick={submit}>
            {editing ? t('common:actions.save') : t('delegations:grant.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
      {grant && (
        <ConfirmDialog
          open={confirmRevoke}
          title={t('delegations:revoke.title')}
          description={t('delegations:revoke.confirm', { name: isolate(grant.delegate.display_name) })}
          confirmLabel={t('delegations:revoke.button')}
          destructive
          busy={revoke.isPending}
          onCancel={() => {
            setConfirmRevoke(false)
          }}
          onConfirm={() => {
            revoke.mutate(grant.id, {
              onSuccess: () => {
                setConfirmRevoke(false)
                onClose()
              },
            })
          }}
        />
      )}
    </Dialog>
  )
}
