/**
 * Pages that stay where they are when the workspace switches: list pages that make sense in
 * every workspace. Any other page (a detail page, a form) goes home, so an item of workspace A
 * never loads as "not found" in B.
 */
const KEEP_ON_SWITCH = new Set([
  '/',
  '/accounts',
  '/accounts/groups',
  '/delegations',
  '/notifications',
  '/profile',
  '/oauth-done',
])

export function keepsOnSwitch(pathname: string): boolean {
  return KEEP_ON_SWITCH.has(pathname)
}
