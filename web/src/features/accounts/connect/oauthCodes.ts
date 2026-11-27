/** The codes the OAuth callback redirects with, each with a sentence in the `accounts` catalog. */
export const OAUTH_RESULT_CODES = [
  'access_denied',
  'provider_error',
  'exchange_failed',
  'scope_missing',
  'discovery_failed',
  'no_accounts',
  'platform_unavailable',
  'state_invalid',
  'account_connected_elsewhere',
] as const

export type OAuthResultCode = (typeof OAUTH_RESULT_CODES)[number]

export const isOAuthResultCode = (v: string | null): v is OAuthResultCode =>
  v !== null && (OAUTH_RESULT_CODES as readonly string[]).includes(v)
