/**
 * Wraps user text in Unicode first-strong isolates so a name in the other direction cannot
 * reorder the sentence around it. For strings; in JSX prefer `<bdi>`.
 */
export function isolate(text: string): string {
  return `⁨${text}⁩`
}
