import { Button } from '@/components/ui/button'

/** Plain error screen shown when the runtime or auth config cannot be loaded; Retry re-runs startup. */
export function StartupError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 p-6 text-center">
      <h1 className="font-display text-3xl font-bold tracking-[-0.02em]">postit could not start</h1>
      <p role="alert" className="text-muted-foreground">
        {message}
      </p>
      <div className="flex justify-center">
        <Button onClick={onRetry}>Try again</Button>
      </div>
    </main>
  )
}
