# B7 Account connection: OAuth — design

Plan 03 phase B7. Source of truth: `!ref/plans/03. publishing roadmap.md` (Phase B7, "Connecting
an account with OAuth", "Credential refresh", "Disconnecting an account", the REST surface rows
marked B7, the `refresh_credentials` and `oauth_state_purge` jobs, the deletion steps) and
`!ref/plans/01. vision and architecture.md` (Grants, `account_grants`, `account_tokens`,
`oauth_states`, stored secrets). This document records how B7 fits the code as it stands after
B3, B4, and B5, and the calls made where the plans leave room. It does not repeat the plans;
read those sections first.

## Intent

B3 connects accounts from pasted credentials. B7 adds the other connect method: an OAuth 2.0
authorization-code flow with PKCE, driven entirely by the agent's `OAuthProviderSpec` and a few
optional agent hooks, so that each reviewed-platform agent in Group F (TikTok, X, YouTube,
LinkedIn, Meta) contributes only its spec, its hooks, and its protocol code. No provider-specific
code lives in `postit-oauth`.

No R1 agent uses OAuth. B7 is in R1 so the web (D4) and mobile (D6) connection screens are
built once against the mock OAuth provider, and F1 then needs no client work.

## Goals

- **Start:** `POST /accounts/connect/{platform}` with `{ return_to }` creates an `oauth_states`
  row and returns `{ authorize_url }`.
- **Callback:** `GET /platforms/{id}/oauth/callback` (no bearer) consumes the state, exchanges
  the code, runs the agent's post-exchange hook, discovers accounts, stores a pending result on
  the state row, and redirects to `return_to` with `result=ok&connect_id=…` or
  `result=error&code=…`.
- **Confirm:** the existing `POST /accounts/connect/{connect_id}/confirm` also accepts OAuth
  connect IDs: same user only, the checklist, `account_connected_elsewhere`, reconnect moves.
- **Credentials:** `PgCredentialStore` serves OAuth tokens and per-account tokens; refresh is
  serialized per grant with a transaction-scoped advisory lock; on-demand refresh after a 401 and
  the `refresh_credentials` cron share one code path; `needs_reconnect` on `invalid_grant`;
  `reconnect_by` stored on the grant and enforced when it passes.
- **Revocation:** a grant is revoked at the provider when its last account leaves it, and in the
  OAuth step of `delete_user`.
- **Platform events:** `POST /platforms/{id}/events/{kind}` (no bearer) calls the agent's
  `handle_event` and applies the deauthorizations it reports.
- **Errors:** clear results for a platform with no deployment config, denied authorization,
  invalid or expired state, expired or revoked refresh tokens, and missing scopes.
- **Housekeeping:** the `oauth_state_purge` cron (hourly) for expired `oauth_states` and
  `pending_connections`, revoking at the provider the tokens of discarded OAuth results first.
- **Tests:** the whole list in the B7 phase, against the mock OAuth provider.

## Non-goals

- **Real OAuth agents.** They arrive in Group F. B7 proves the engine with `mock_oauth` only.
- **Client screens.** The web and mobile checklist, `/oauth-done` handling, and translations of
  the callback result codes are D4 and D6. B7 regenerates the OpenAPI contract, the TypeScript
  types, and the Dart client, and adds the new problem `code`s to the web and mobile error
  message maps (as B6 did), nothing more.
- **Notifications.** `postit-notify` arrives in D1. The owner notifications the plan describes
  (`needs_reconnect`, `reconnect_by` warnings at 7 days and 1 day, revoked by the platform) are
  not sent in B7; see "Deferred to D1" below.
- **Delivery parking.** Deliveries are not touched by refresh or revocation; the D1 worker parks
  them with `account_needs_reconnect` when they come due.
- **`account_preflight`.** It is a publishing cron (D1).
- **Registering `mock_oauth` in a running server.** It stays test-only, as B5 left it. D4
  decides how the web screens are exercised by hand. *(My call: registering it needs a fake
  authorize page in the server, which is D4's concern.)*

## Anti-goals

- An attacker can start a connection and get a victim's platform account attached to the
  attacker's postit user. The confirm step exists to prevent exactly this.
- Any token, refresh token, authorization code, PKCE verifier, `state`, `connect_id`, client
  secret, or raw token-endpoint body in a log, an error, a problem response, a redirect other
  than the one `connect_id` itself, an audit event, or the OpenAPI examples.
- `return_to` used as an open redirect.
- Revoking at the provider a grant that a newer authorization still depends on.
- A refresh-token rotation lost because two processes refreshed at once.
- Platform-specific branches in `postit-oauth`, `postit-api`, or `postit-server`.

## Constraints

- One migration file: schema changes edit `backend/crates/data/migrations/0001_init.sql` in
  place (memory: single migration policy).
- `postit-http` is the only place that builds `reqwest` clients. Token, refresh, and revoke calls
  use the default (platform hosts) client from the `HttpHandles` the server already builds.
- Agent crates never depend on `postit-oauth`. Everything an agent needs reaches it through
  `postit-core` traits in its context.
- Every secret column is BYTEA with `COMMENT … 'postit:sealed'` and is listed in
  `SEALED_COLUMNS`; the sealed-column and no-plaintext tests must keep passing.
- The quality gates in `CLAUDE.md`, plus `cargo xtask openapi --check`.
- **Sequencing with B6.** B6 (worktree `b6-delegation`) changes `0001_init.sql`,
  `postit-data`'s `accounts.rs`, `api/src/routes/accounts.rs`, and the `oauth` tests. B7 should
  branch from `stage` after B6 merges. If it starts earlier, it rebases onto B6 before its final
  review.

## Decisions

### Contract additions in `postit-core`

The current contract lacks three things the B7 flow needs. All are additive with defaults, so
the mock and the B5 conformance suite keep compiling.

1. **`OAuthProviderSpec` token-request quirks.** Add:
   - `client_id_param` (default `"client_id"`; TikTok uses `client_key`),
   - `extra_token_params: Vec<(String, String)>`.

   The forced login screen already fits in `extra_authorize_params`. Anything else a provider
   does differently is handled by the post-exchange hook, which sees the raw token response.

2. **Post-exchange hook on `Agent`:**
   `async fn after_exchange(&self, ctx: &ExchangeContext, exchange: TokenExchange) ->
   Result<GrantExchange, AgentError>`, default: the tokens unchanged and nothing else.
   - `ExchangeContext`: config, HTTP handles, clock (like `CredentialCheckContext`).
   - `TokenExchange`: the parsed `OAuthTokens` plus the raw token response JSON in a redacting
     wrapper (`Debug` prints `[redacted]`), so an agent can read fields such as TikTok's
     `open_id` or Meta's user ID.
   - `GrantExchange { tokens, remote_user_id: Option<String>, reconnect_by:
     Option<DateTime<Utc>>, account_tokens: BTreeMap<String /* remote account id */,
     AccountToken> }`. `AccountToken { access_token: SecretString, expires_at }` with a
     redacting `Debug`. Meta's long-lived exchange and Page token derivation live here (F5).
   - The same hook runs after each refresh? **No.** Refresh only replaces the grant tokens;
     an agent that must re-derive per-account tokens does it in its client on demand (F5's
     concern). *(My call: keeps refresh a pure token operation.)*

3. **`handle_event` reports deauthorizations.** Change its return type to
   `EventOutcome { response: EventResponse, deauthorized: Vec<String> /* remote user ids */ }`.
   A data-deletion request is a deauthorization whose `response` carries the body the platform
   expects. `EventContext` gains the HTTP handles (Meta verifies nothing remotely, but X's CRC
   and others may need them; cheap to add now).

4. **`CredentialStore` additions:**
   - `async fn refresh(&self, stale: &OAuthTokens) -> Result<OAuthTokens, AgentError>`: the
     on-demand path after a 401. Implemented by the engine (below); agents call it instead of
     talking to the token endpoint themselves.
   - `async fn account_token(&self, account: &AccountRef) -> Result<Option<AccountToken>,
     AgentError>`.
   - `store_refreshed` stays for agents that refresh by a non-standard call, and gains the
     OAuth arm in `PgCredentialStore` (sealed write under the same grant lock).
   - The B5 in-memory `CredentialStore` fake in `agent-kit`'s testkit implements both.

The conformance suite gains checks for the new defaults: `after_exchange` on a credentials agent
is never called; an OAuth agent's config schema declares `client_id` (string) and
`client_secret` (string, `x-postit-secret`).

### The OAuth engine (`postit-oauth`)

A small hand-written client over `postit-http`: build the authorize URL, exchange a code,
refresh, revoke (RFC 7009 form post with `token` and `token_type_hint`). PKCE S256 verifier
and challenge from 32 random bytes, base64url without padding. Client auth per
`ClientAuth`; client ID and secret come from the platform's resolved `[platforms.<id>]` table
(`client_id`, `client_secret`). *(My call: the `oauth2` crate was considered and rejected; it
wants its own HTTP client or an adapter, and its typed responses fight the quirks above. The
protocol surface is three form posts, and `postit-http` already gives retry classification and
redaction. This is the "dependency fights the architecture" exception to the prefer-libraries
rule.)*

Token-endpoint errors are classified from the OAuth `error` field only: `invalid_grant` means
the grant is dead; `invalid_client` and `unauthorized_client` mean the deployment config is
wrong (logged as such, surfaced as `platform_unavailable`); 5xx and transport errors are
retryable. The response body is never logged.

The redirect URI is `{server.public_url}/api/v1/platforms/{id}/oauth/callback`, sent on
authorize and on code exchange.

### Start

`AccountService::start_oauth(scope, platform, return_to)`:

- Owner only (`owner_only` for a delegate). Catalog resolution gives the existing
  `platform not found` / `disabled` / `not configured` errors; a credentials platform answers
  `ConnectMethodUnsupported`.
- `return_to` must exactly equal one entry of the allowlist (no prefix match, no query, no
  fragment); otherwise 422 with the new problem code `return_to_invalid`. The allowlist, built
  once at startup: `{app.public_url}/oauth-done`, `{scheme}://oauth-done` where the scheme is
  `postit-dev`, `postit-qa`, or `postit` by environment (matching
  `mobile/android/app/build.gradle.kts`), plus `oauth.return_to_allowlist`.
- Inserts the `oauth_states` row: SHA-256 of a random 32-byte state, sealed PKCE verifier,
  `return_to`, `user_id` = the actor (who is the owner here), 10-minute expiry.

### Callback

`GET /platforms/{id}/oauth/callback?code&state` or `?error&state`. No bearer, per-IP rate
limit, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`. Always answers 303.

1. Look up the state by hash and **consume** it in the same statement (set a `consumed_at`, or
   delete and re-insert as the pending result; builder's choice). Unknown, consumed, expired,
   or a platform mismatch: redirect to `{app.public_url}/oauth-done?result=error&code=state_invalid`,
   because no trustworthy `return_to` exists. *(My call.)*
2. `error=access_denied` (or any provider error) on a valid state: redirect to its `return_to`
   with `code=access_denied` (other provider errors: `code=provider_error`) and delete the row.
3. Exchange the code with the verifier. Failure: `code=exchange_failed`.
4. Missing scopes: when the token response lists scopes, every scope the spec requests must be
   present; otherwise `code=scope_missing`. When it lists none, assume granted.
5. Run `after_exchange`, then build a client over an in-memory credential store holding the
   exchanged tokens and call `discover_accounts`. Failure: `code=discovery_failed`. No accounts:
   `code=no_accounts`.
6. Store the pending result on the state row: `connect_id_hash`, sealed `grant_tokens` (the
   `GrantExchange` minus the discovered list), `discovered`, and the remaining expiry
   unchanged. Redirect to `return_to?result=ok&connect_id=…`.

Every error path creates nothing. An error before or at the exchange holds no tokens and deletes
the row; an error after a successful exchange (steps 4 and 5) **discards** it instead, keeping the
sealed tokens for the purge to revoke (see "Discarded results"). No database connection is held
during a platform call (same rule as B3's `verify`).

The platform must still be available at callback time; an admin switching it off mid-flow gives
`code=platform_unavailable`.

### Reading a pending result

The redirect carries only `result` and `connect_id`, so the client needs a way to get the
discovered accounts for the checklist. B7 adds `GET /accounts/connect/{connect_id}` (owner
only), returning `{ platform, expires_at, discovered[] }` without consuming the result. It
serves both pending OAuth results and B3's pending credential connections, so the clients have
one code path. A read by any user other than the one who started the connection ends the
result (an OAuth result is discarded, a credential one deleted) and answers 404, as confirm does; an expired or unknown ID answers 404. Accounts never
go into the redirect URL. *(My call: the plan's REST table has no such route, but D4's
`/oauth-done` screen cannot show a checklist without it.)*

### Confirm

`AccountService::confirm` looks up the presented `connect_id` hash in `pending_connections`,
then in `oauth_states`. For an OAuth result: the row's `user_id` must equal the caller
(`scope.owner`, and the route is owner-only); otherwise the row is discarded and the answer is
404, as B3 does (an expired result is discarded too). Then in one transaction: delete the state
row (discard it instead when nothing gets connected), create the `oauth` grant (sealed
tokens, `remote_user_id`, `expires_at`, `reconnect_by`, `scopes`), upsert the chosen accounts
with the grant's scopes, write `account_tokens` for chosen accounts that have one, delete
previous grants left with no accounts, and record `account_connected` per account.

`connect_in_tx` is generalized over the grant kind instead of hard-coding `Credentials`.

**Moving an account never revokes its previous grant at the provider.** The previous grant and
the new one usually belong to the same remote user and app, and several providers (Google among
them) revoke every token of that pair when one is revoked. The old row is only deleted.

### Refresh

One function, `refresh_grant(grant_id, stale)`, used by the cron and by
`PgCredentialStore::refresh`:

1. Begin a transaction and take `pg_advisory_xact_lock` on a key derived from the grant ID. The
   key must not collide with the fixed keys in `postit-data::advisory_lock`; the two-`int4`
   form with a reserved namespace is the obvious choice (builder's call).
2. Re-read the grant. If its tokens differ from `stale` (another process refreshed), return
   them. If it is not `active`, return `Authentication`.
3. Call the token endpoint with the refresh token, inside the transaction. Write the new sealed
   tokens and `expires_at` (keeping the old refresh token when the provider did not rotate it).
   Commit.
4. `invalid_grant`: set the grant and its non-disconnected accounts to `needs_reconnect`, record
   `account_needs_reconnect` per account, commit, return `Authentication`. Retryable errors:
   roll back, return the transport error; the next cron run tries again.

`PgCredentialStore::credentials()` for an OAuth grant whose access token has already expired
refreshes first. A grant with no refresh token whose access token has expired goes to
`needs_reconnect` inside `refresh_grant`, whichever path (cron or on demand) reaches it first.

**`refresh_credentials` cron** (every 5 minutes, `oauth` queue owner, worker only): active
`oauth` grants with `expires_at` within 15 minutes and a refresh token, each refreshed through
the same function, one at a time; plus active grants whose `reconnect_by` has passed, set to
`needs_reconnect` with `reconnect_warned = 'expired'`.

### Revocation and disconnect

`disconnect` already revokes and deletes an unused grant (B3). B7 makes the OAuth arm real
through one function, `revoke_grant`, used by disconnect and the deletion step:

- **Who revokes.** When `spec.revoke_url` is set, the engine sends the RFC 7009 request (the
  refresh token, or the access token when there is none; client auth and `client_id_param` as
  for the token endpoint). When it is not set, the engine calls the agent's
  `AgentClient::revoke()`, which stays in the contract for providers whose revocation is not
  RFC 7009 (Meta's `DELETE /me/permissions`). Credentials grants keep calling
  `client.revoke()` as in B3. *(My call.)*
- **Skip the revoke** when another active grant of the same platform and `remote_user_id`
  exists (any owner), or a live (unexpired, undiscarded) pending result of them does, for the
  reason given under Confirm. A pending result in another tab would otherwise lose its tokens
  on a provider that revokes per user and app pair.
- **A grant with no `remote_user_id` is never revoked at the provider.** Without the identity
  the skip rule cannot be applied, and a wrong revoke breaks a newer authorization (an
  anti-goal). The row is still deleted. Such grants also cannot match a deauthorize event.
  Agent docs (the `mock_oauth` template) say an OAuth agent should return `remote_user_id`
  from `after_exchange`. *(My call: cautious over mandatory, so an agent is not blocked by a
  provider that hides the user ID.)*
- Best effort, as now: a failure never undoes the disconnect.

### Discarded results

*(Decided after the plan review.)* Tokens postit obtained but will not use are revoked at the
provider, never only dropped: otherwise the authorization stays live there, listed under the
user's connected apps and, on some providers, skipping the next consent screen. A result is
discarded by a callback failure after the exchange, a read or confirm by another user, a
confirm of an expired result, a confirm that connects nothing (no account chosen, or every
chosen account connected elsewhere), or plain expiry.

Request paths never call a provider to revoke. They **discard** the row: clear its
`connect_id_hash` (so no one can read or confirm it), expire it, and keep its sealed tokens and
its `remote_user_id`, which `oauth_states` now stores in clear (as `account_grants` does) for the
skip rule. The hourly `oauth_state_purge` takes each expired row that holds tokens (deleting it
as it takes it, so no row is revoked twice), revokes its tokens under the same skip rule, then
deletes the remaining expired rows. A row without `remote_user_id`, or whose platform no longer
resolves, is deleted without a revoke. The OAuth deletion step revokes the user's own pending
results before their row deletion would cascade them away.

The cost: sealed tokens of a discarded result stay in the database, and in its backups, for up
to an hour. *(Rejected: exchanging the code only at confirm, since discovery needs the tokens
for the checklist; revoking inline in request paths, which would put provider calls and their
latency on the callback and confirm.)*

### Platform events

`POST /platforms/{id}/events/{kind}`: no bearer, per-IP rate limit, raw body up to a fixed limit
(64 KiB; builder may make it a setting). The route builds the `InboundEvent` and calls
`handle_event`. `Unsupported` → 404. A verification failure (the agent's
`Authentication` error) → 401 with no detail. For each `deauthorized` remote user ID, in one
transaction: every grant of that platform and `remote_user_id` becomes `revoked` with its
credential cleared, its `account_tokens` rows are deleted, its non-disconnected accounts become
`revoked`, and `account_revoked_by_platform` is recorded per account. The agent's `response` is
returned as is.

`account_grants.credential` becomes nullable so a revoked grant holds no secret.
`PgCredentialStore` treats a null credential as "reconnect".

### User deletion

`postit-identity` gains the `UserDeletionStep` trait plan 03 describes (idempotent, resumable,
`async fn run(&self, user) -> Result<(), JobError>`), and `DeleteUserHandler` runs the
registered steps, in order, between its step 1 and step 2. `postit-server` registers the OAuth
step: revoke each of the user's OAuth grants not yet `revoked` (same skip rule; a
`needs_reconnect` grant past its `reconnect_by` may still be live at the provider), marking each
`revoked` with its credential cleared after its call so a retry does not repeat it. The cascade then removes the rows. D1 registers the
publishing and media steps (the B4 deferred entry already points there).

### New codes, audit, and config

- Problem code: `return_to_invalid`. Callback result codes (query values, not problem codes):
  `access_denied`, `provider_error`, `state_invalid`, `exchange_failed`, `scope_missing`,
  `discovery_failed`, `no_accounts`, `platform_unavailable`, and on confirm the existing
  `account_connected_elsewhere`. Documented in the OpenAPI description of the callback route.
- Audit: `account_needs_reconnect` (exists), `account_revoked_by_platform` (add if missing, with
  its web audit-kind label in English and Arabic).
- Config: `oauth.return_to_allowlist` stops being "reserved". Rate limits for the two
  unauthenticated routes: a new `[rate_limit.oauth_callbacks]` table (builder may reuse an
  existing limiter if one fits).

### Mock OAuth provider and `mock_oauth`

- `MockOAuthProvider` (agent-kit testkit) gains an approval helper that returns a code bound to
  the PKCE challenge and redirect URI the authorize URL carried, and the token endpoint checks
  the verifier, the redirect URI, and client authentication. It can return a `scope` list (to
  test `scope_missing`) and a denial.
- `mock_oauth` implements `after_exchange` (a scripted `remote_user_id`, `reconnect_by`, and
  account tokens, all optional) and `handle_event("deauthorize")`, verifying an HMAC-SHA256
  signature over the raw body with the configured `client_secret`, so the event route's
  verification path is exercised. Its discovery returns a scriptable number of accounts so
  "several accounts from one grant" is testable.

### Docs

- `docs/configuration.md`: `oauth.return_to_allowlist`, the callback URL, the new rate limit.
- `docs/deployment.md`: registering a platform developer app (redirect URI to give the provider)
  and the dev tunnel fallback (cloudflared or ngrok as `server.public_url`) for providers that
  reject `postit.local`. **Phase-start check:** the per-provider redirect URI rules are recorded
  as a short table there, from each provider's current docs; each F phase re-verifies its own
  row at its start.
- `docs/security.md`: the confirm step's rationale and the revoke-skip rule.
- `CLAUDE.md` project state: B7 built.

### Deferred to D1 (goes into `<plan>-deferred.md` at plan end)

- Owner notifications for `needs_reconnect`, `account_revoked_by_platform`, and the
  `reconnect_by` warnings at 7 days and 1 day (the `reconnect_warned` stages `7d` and `1d`).
  They need `postit-notify`. Trigger: D1.

## Tests

From the phase, all against `MockOAuthProvider` and `mock_oauth`, at the service layer and at
HTTP where a route is involved:

- PKCE: the challenge on the authorize URL matches the verifier sent on exchange; a wrong
  verifier fails the exchange.
- State: unknown, expired, reused (single use), and wrong-platform states; each creates nothing.
- `return_to`: exact allowlist entries accepted; prefix, extra path, query, fragment, other
  scheme, and other host rejected with `return_to_invalid`.
- Confirm: a result confirmed by a different user is refused with 404 and discarded; an expired
  result creates nothing; the attacker-initiated scenario (attacker starts, victim authorizes,
  attacker cannot confirm without the `connect_id`, victim's confirm creates nothing for the
  attacker) attaches nothing; the checklist adds only chosen accounts; an account connected by
  another owner is skipped and only counted.
- Reading a pending result: the owner who started it reads the checklist without consuming it;
  another user's read deletes it and gets 404; works for both OAuth and credential results.
- Grants: a grant without `remote_user_id` is deleted but not revoked; revocation goes to
  `revoke_url` when set and to the agent's `revoke()` otherwise;
  several accounts from one grant share its token; disconnecting one leaves the grant
  unrevoked; disconnecting the last revokes it (provider sees the revoke); the revoke-skip rule
  when another grant of the same remote user exists; moving an account does not revoke.
- Refresh: before expiry via the cron; on demand after a 401; rotation stored; no rotation keeps
  the old refresh token; `invalid_grant` and an expired refresh token set `needs_reconnect` on
  the grant and its accounts with audit events; a 5xx leaves the grant active; concurrent
  refresh from two pools makes exactly one token request; `reconnect_by` passed sets
  `needs_reconnect`.
- Events: a valid deauthorize revokes the matching grants and accounts, clears the credential,
  deletes account tokens, and records audit events; a bad signature returns 401 and changes
  nothing; an agent without `handle_event` returns 404.
- Errors: missing platform config, denied authorization, scope missing, exchange failure,
  discovery finding nothing.
- Deletion: the OAuth deletion step revokes active grants and is idempotent across retries.
- Purge: expired `oauth_states` and `pending_connections` are deleted, live ones stay; discarded
  results are revoked first, except under the skip rule or without a `remote_user_id`.
- Redaction: the shared no-secret-leak harness covers logs, errors, problem responses, audit
  details, and the callback redirects (only `connect_id` appears, only on the success redirect).
- Access: every new owned-data route has the four-caller isolation tests (owner, delegate at
  each level, delegate limited to other accounts, unrelated user); start and confirm are
  owner-only.

## Left to the builder

- Module and type names inside `postit-oauth`, and whether the engine is its own module or
  sits beside `connect.rs`.
- How the state row is consumed atomically (a `consumed_at` column or delete-then-insert).
- The advisory-lock key derivation, within the collision rule above.
- DTO and route handler names; the exact OpenAPI wording of the callback result codes.
- Whether the event-route body limit is a constant or a setting.
