import { Navigate, useLocation } from 'react-router'
import { Button } from '@/components/ui/button'
import { useSession } from '@/auth/session'

export function LoginPage() {
  const { status, notice, signIn } = useSession()
  const from = (useLocation().state as { from?: string } | null)?.from ?? '/'
  if (status === 'signedIn') return <Navigate to={from} replace />
  return (
    <main className="mx-auto flex min-h-screen max-w-sm flex-col justify-center gap-4 p-6 text-center">
      <h1 className="text-3xl font-semibold">postit</h1>
      {notice ? (
        <p role="alert" className="text-sm text-destructive">
          {notice}
        </p>
      ) : (
        <p className="text-muted-foreground">Sign in to continue.</p>
      )}
      <Button
        disabled={status === 'loading'}
        onClick={() => {
          signIn(from)
        }}
      >
        Sign in
      </Button>
    </main>
  )
}
