import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
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
import { useMe } from './useMe'

export function ProfilePage() {
  const me = useMe()
  const api = useApi()
  const { signOut } = useSession()
  const [open, setOpen] = useState(false)
  const [typed, setTyped] = useState('')
  const del = useMutation({
    mutationFn: (display_name: string) => ensureOk(api.DELETE('/api/v1/me', { body: { display_name } })),
    onSuccess: signOut,
  })
  if (!me.data) return <p className="text-muted-foreground">Loading…</p>
  const name = me.data.display_name
  return (
    <div className="max-w-lg space-y-6">
      <div className="space-y-1.5">
        <div className="font-mono text-xs tracking-[0.06em] text-faint uppercase">Account</div>
        <h1 className="font-display text-4xl font-bold tracking-[-0.02em]">Profile</h1>
      </div>
      <dl className="grid grid-cols-[8rem_1fr] gap-2 text-sm">
        <dt className="text-muted-foreground">Name</dt>
        <dd>{name}</dd>
        <dt className="text-muted-foreground">Email</dt>
        <dd>{me.data.email ?? '—'}</dd>
        <dt className="text-muted-foreground">Role</dt>
        <dd>{me.data.role}</dd>
      </dl>
      <p className="text-sm text-muted-foreground">
        Your name and email come from your identity provider.{' '}
        {me.data.account_url && (
          <a className="underline" href={me.data.account_url} target="_blank" rel="noreferrer">
            Manage account
          </a>
        )}
      </p>
      <div className="flex gap-2">
        <Button variant="outline" onClick={signOut}>
          Sign out
        </Button>
        <Button
          variant="destructive"
          onClick={() => {
            setOpen(true)
          }}
        >
          Delete my postit account
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
            <DialogTitle>Delete your account?</DialogTitle>
            <DialogDescription>This cannot be undone. Type your name to confirm.</DialogDescription>
          </DialogHeader>
          <Label htmlFor="confirm-name">Type your name ({name})</Label>
          <Input
            id="confirm-name"
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
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={typed !== name || del.isPending}
              onClick={() => {
                del.mutate(name)
              }}
            >
              Delete account
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
