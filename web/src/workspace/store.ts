/** Where the tab acts: the owner whose workspace it works in, or `null` for the user's own. Per tab. */
export const ACT_AS_KEY = 'postit.actAs'

// Kept beside sessionStorage so a browser that blocks storage still switches workspaces.
let memory: string | null = null
const listeners = new Set<() => void>()
const endedListeners = new Set<() => void>()

export function getActAs(): string | null {
  try {
    return sessionStorage.getItem(ACT_AS_KEY)
  } catch {
    return memory
  }
}

export function setActAs(owner: string | null): void {
  memory = owner
  try {
    if (owner === null) sessionStorage.removeItem(ACT_AS_KEY)
    else sessionStorage.setItem(ACT_AS_KEY, owner)
  } catch {
    // Storage blocked: the in-memory value carries the tab.
  }
  listeners.forEach((fn) => {
    fn()
  })
}

export function subscribeActAs(fn: () => void): () => void {
  listeners.add(fn)
  return () => {
    listeners.delete(fn)
  }
}

/** Called when the server says the grant behind the active workspace ended. */
export function notifyDelegationInvalid(): void {
  endedListeners.forEach((fn) => {
    fn()
  })
}

export function subscribeDelegationInvalid(fn: () => void): () => void {
  endedListeners.add(fn)
  return () => {
    endedListeners.delete(fn)
  }
}
