/** Wraps an async function so concurrent callers share one in-flight promise. */
export function singleFlight<T>(fn: () => Promise<T>): () => Promise<T> {
  let inFlight: Promise<T> | null = null
  return () => {
    inFlight ??= fn().finally(() => {
      inFlight = null
    })
    return inFlight
  }
}
