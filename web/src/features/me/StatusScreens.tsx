import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { useSession } from '@/auth/session'

function Screen({ title, children }: { title: string; children: ReactNode }) {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 p-6 text-center">
      <h1 className="text-2xl font-semibold">{title}</h1>
      {children}
    </main>
  )
}

export function PendingScreen({ onRefresh }: { onRefresh: () => void }) {
  const { signOut } = useSession()
  return (
    <Screen title="Waiting for approval">
      <p className="text-muted-foreground">
        Your account is waiting for approval. An admin has been notified; you can come back later.
      </p>
      <div className="flex justify-center gap-2">
        <Button onClick={onRefresh}>Refresh</Button>
        <Button variant="outline" onClick={signOut}>
          Sign out
        </Button>
      </div>
    </Screen>
  )
}

export function DisabledScreen() {
  const { signOut } = useSession()
  return (
    <Screen title="Account unavailable">
      <p className="text-muted-foreground">
        Your account is disabled or is being deleted. Contact an admin if you think this is a mistake.
      </p>
      <div className="flex justify-center">
        <Button variant="outline" onClick={signOut}>
          Sign out
        </Button>
      </div>
    </Screen>
  )
}

export function ErrorScreen({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { signOut } = useSession()
  return (
    <Screen title="Something went wrong">
      <p className="text-muted-foreground">{message}</p>
      <div className="flex justify-center gap-2">
        <Button onClick={onRetry}>Try again</Button>
        <Button variant="outline" onClick={signOut}>
          Sign out
        </Button>
      </div>
    </Screen>
  )
}

export function Forbidden() {
  return (
    <div className="p-6">
      <h1 className="text-xl font-semibold">Not allowed</h1>
      <p className="text-muted-foreground">You are not allowed to see this page.</p>
    </div>
  )
}

export function Loading() {
  return (
    <div className="p-6 text-muted-foreground" role="status">
      Loading…
    </div>
  )
}
