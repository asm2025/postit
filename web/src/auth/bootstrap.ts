import type { UserManager } from 'oidc-client-ts'

interface Win {
  self: unknown
  top: unknown
  location: { pathname: string }
}

/** True in the hidden iframe the silent sign-in opens: framed, on the callback route. */
export function isSilentFrame(win: Win): boolean {
  return win.self !== win.top && win.location.pathname === '/auth/callback'
}

/** In a silent frame, hand the response to the parent window and render nothing else. */
export async function completeSilentFrame(um: UserManager): Promise<void> {
  await um.signinSilentCallback()
}
