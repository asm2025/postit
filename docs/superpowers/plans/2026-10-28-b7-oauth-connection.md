# B7: Account Connection through OAuth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect accounts through any agent-provided OAuth 2.0 spec (authorization code with PKCE, server-side callback, authenticated confirm), then refresh, serve, revoke, and deauthorize their credentials, all proven against the mock OAuth provider with no platform code.

**Architecture:**
- `postit-core` gains the contract pieces the flow needs: token-request quirks on `OAuthProviderSpec`, the `Agent::after_exchange` hook (`ExchangeContext`, `TokenExchange`, `GrantExchange`, `AccountToken`), `EventOutcome` from `handle_event`, and `CredentialStore::refresh` and `account_token`.
- `postit-data` gains `OAuthStateRepo`, the system-side `GrantRepo` calls for refresh, revocation, and events, a nullable `account_grants.credential`, and the purge query.
- `postit-oauth` gains a hand-written OAuth client over `postit-http` (`engine`), the start, callback, pending-read, and confirm flow, `refresh_grant` behind `PgCredentialStore`, `revoke_grant`, platform-event handling, and the `refresh_credentials` and `oauth_state_purge` jobs.
- `postit-identity` gains `UserDeletionStep`; `postit-server` registers the OAuth step, the jobs, and the allowlist; `postit-api` adds four routes.

**Tech Stack:** Rust 2024, axum 0.8, sqlx (Postgres), `postit-http` (reqwest underneath), wiremock (tests), utoipa. No new workspace dependencies: `hmac` 0.12, `sha2` 0.10, `emixcrypto`, `base64`, and `rand` are already workspace dependencies; crates add them from `[workspace.dependencies]` as needed.

**Spec:** `docs/superpowers/specs/2026-10-28-b7-oauth-connection-design.md` (approved). It argues from `!ref/plans/03. publishing roadmap.md` (Phase B7, "Connecting an account with OAuth", "Credential refresh", "Disconnecting an account", jobs, deletion steps) and `!ref/plans/01. vision and architecture.md` (Grants, `account_grants`, `account_tokens`, `oauth_states`). Read all three first. The spec's "My call" items are decided and not reopened.

## Global Constraints

- **Start after B6 merges into `stage`** (B6 edits `0001_init.sql`, `data/src/accounts.rs`, `api/src/routes/accounts.rs`, and the `oauth` tests). Branch `b7-oauth-connection` from that `stage`. Scan `docs/superpowers/plans/*-deferred.md` again at start for entries whose trigger is B7 (none at plan time).
- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests).
- Single migration policy: edit `backend/crates/data/migrations/0001_init.sql` in place.
- `postit-oauth` contains no platform-specific code; agent crates never depend on `postit-oauth`.
- Token, refresh, and revoke calls go through the `postit_http::StandardClient` downcast from the existing `AccountServiceDeps.http.default` (as `postit-agent-kit::http::standard` does). No new `reqwest::Client` and no second HTTP field in `AccountServiceDeps`.
- Retry idempotency (`postit_http::Idempotency`) per call: the code exchange is `NonIdempotent` (a code is single use, so a resend after a lost response can only fail); a refresh is `NonIdempotent` when `spec.refresh.rotates` (a resent rotated refresh token fails, and some providers revoke the whole token family on reuse) and `Idempotent` otherwise; a revoke is `Idempotent` (RFC 7009). An outcome-unknown failure maps to `TokenError::Transport` (retryable), never to `needs_reconnect`.
- URLs are built with `Url::join` on a base whose path ends in `/`, never by string formatting: `Url` serializes an empty path as `/`, so `format!("{url}/oauth-done")` yields `//oauth-done`.
- Callback deadline: steps 3–5 of the callback (exchange, `after_exchange`, discovery) run under one `tokio::time::timeout` of `AccountServiceDeps.callback_deadline` (server value `postit_oauth::CALLBACK_DEADLINE = 60s`). On expiry the callback ends the row as any failure at that step does (see "Discarded results") and redirects with the code of the step that was running (`exchange_failed` or `discovery_failed`). The callback route runs under its own timeout `CALLBACK_TIMEOUT = CALLBACK_DEADLINE + 30s`, so the API's 30-second default `request_timeout` never answers a browser with a 408 problem after the state was consumed.
- **Discarded results** (decided after the plan review): tokens postit obtained but will not use are revoked at the provider, never just dropped, because the authorization otherwise stays live there (listed under the user's connected apps, and on some providers it skips the next consent screen). One path does every such revoke: the `oauth_state_purge` job. Request paths never call a provider to revoke; they **discard** the row instead (`connect_id_hash = NULL`, `expires_at = LEAST(expires_at, now)`, sealed `grant_tokens` and the clear `remote_user_id` kept), so the next purge (hourly) revokes and deletes it. Discarded: a callback failure after a successful exchange (`scope_missing`, `discovery_failed`, `no_accounts`, a deadline after the exchange); a pending read or confirm by another user; a confirm of an expired result; a confirm that connects nothing (no account chosen, or every chosen account connected elsewhere); a result that simply expires. A callback failure before or at the exchange holds no tokens and deletes the row. The user's own pending results are revoked by the OAuth deletion step, since the user's row deletion cascades them away.
- **Revoke-skip rule** (`remote_user_in_use`), used by the purge and by `revoke_grant`: no provider call when the tokens carry no `remote_user_id`, when an `active` grant of the same platform and remote user exists (any owner), or when an unexpired, undiscarded pending result of the same platform and remote user exists (another tab's connection, which a provider such as Google would otherwise lose, since it revokes every token of a user and app pair together). The row being revoked is excluded from the check.
- No secret in logs, errors, problem bodies, audit details, or redirects: access and refresh tokens, codes, PKCE verifiers, `state`, client secrets, raw token responses. `connect_id` appears only in the success redirect. Token-endpoint failures log the OAuth `error` code only.
- TTLs: state and pending result 10 minutes (`STATE_TTL = Duration::minutes(10)`); refresh window 15 minutes; `refresh_credentials` schedule `0 */5 * * * *`; `oauth_state_purge` schedule `0 0 * * * *` (seconds-first cron, queue `maintenance`).
- Redirect URI: `{server.public_url}/api/v1/platforms/{id}/oauth/callback`. Allowlist: `{app.public_url}/oauth-done`, `{scheme}://oauth-done` with scheme `postit-dev` (development), `postit-qa` (qa), `postit` (production), plus `oauth.return_to_allowlist`; exact string match. The built-in entries come from `Url::join`; the configured entries are parsed with `Url::parse` at startup and kept in their serialized form, so an entry written `https://Example.com/done` matches the canonical `https://example.com/done` a client sends. The candidate itself is compared as the raw string, never normalized.
- Problem code (new): `return_to_invalid` (422). Callback result codes (query values): `access_denied`, `provider_error`, `state_invalid`, `exchange_failed`, `scope_missing`, `discovery_failed`, `no_accounts`, `platform_unavailable`.
- Audit kinds: `account_connected` (exists), `account_needs_reconnect` (exists), `account_revoked_by_platform` (add).
- The unauthenticated callback and event routes are covered by the existing per-IP `rate_limit.unauthenticated` bucket (`api/src/middleware.rs::ip_rate_limit`); no new limiter. *(Resolves the spec's "builder may reuse".)*
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=D:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data` (and from `backend/crates/oauth` if it gains macros).
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`. American English everywhere.

## Review Focus

1. **The user opens the authorize URL twice, or the browser replays the callback.** The second callback with the same `state` must redirect `state_invalid` to `{app.public_url}/oauth-done` and must not touch the first pending result. Task 7 test `callback_replay_does_not_touch_the_pending_result`.
2. **Provider redirects with `error=access_denied` and no `code`, or with both `code` and `error`.** Treat any `error` as failure (`access_denied` or `provider_error`), delete the row, never exchange. Task 7 test `callback_with_error_and_code_never_exchanges`.
3. **The access token expired while the worker was down for hours and the provider issued no refresh token.** `credentials()` and the next cron run must both answer `Authentication` and set `needs_reconnect` (one code path, `refresh_grant`), never call the token endpoint and never loop. Task 9 test `expired_without_refresh_token_needs_reconnect`.
4. **The same remote account is connected by owner A, disconnected, then connected by owner B through OAuth while A's old grant row lingers.** B's connect succeeds; A's old grant is not revoked at the provider because B's grant has the same `remote_user_id`. Task 10 test `revoke_skipped_when_another_owner_holds_the_remote_user`.
5. **`return_to` with a trailing slash, different case in the host, or `%2F`-encoded path.** All rejected (exact match), with `return_to_invalid`. Task 7 test `return_to_near_misses_are_rejected`.
6. **The provider is slow and the callback outlives the API's request timeout.** The browser must still get a 303 with a result code, never a 408 problem body, and the consumed state must leave no pending result (a row that already holds tokens is discarded for the purge to revoke). Task 7 test `callback_past_deadline_redirects`.
7. **The cron meets grants that stay due** (a provider answering 5xx, a grant without a refresh token that has not expired yet). One run must visit each due grant once and finish. Task 9 test `cron_does_not_loop_on_grants_that_stay_due`.
8. **An attacker starts a connection and the victim completes it, or a user abandons the checklist.** Nothing attaches to anyone, and the victim's or user's authorization does not stay live at the provider: the result is discarded and the purge revokes it, unless the remote user is in use by an active grant or another live pending result. Task 8 test `attacker_initiated_connection_attaches_nothing` and Task 9 tests `purge_revokes_discarded_results` and `purge_skips_results_whose_remote_user_is_in_use`.

---

### Task 1: Contract additions in `postit-core`

**Files:**
- Modify: `backend/crates/core/src/oauth.rs`, `backend/crates/core/src/agent/client.rs`, `backend/crates/core/src/agent/context.rs`, `backend/crates/core/src/agent/event.rs`, `backend/crates/core/src/agent/mod.rs` (re-exports and fakes), `backend/crates/core/src/lib.rs`
- Create: `backend/crates/core/src/agent/exchange.rs`
- Modify callers so the workspace compiles: `backend/crates/agents/mock/src/lib.rs` (`handle_event` signature if implemented), `backend/crates/agent-kit/src/testkit/fakes.rs`, `backend/crates/agent-kit/src/testkit/context.rs`
- Test: unit tests in the files above

**Interfaces:**
- Produces:
  - `OAuthProviderSpec.client_id_param: String` (default `"client_id"`), `.with_client_id_param(impl Into<String>)`; `OAuthProviderSpec.extra_token_params: Vec<(String, String)>`, `.with_extra_token_param(name, value)`.
  - `ExchangeContext { config: serde_json::Value, http: HttpHandles, clock: Arc<dyn Clock> }`.
  - `RawTokenResponse` (wraps `serde_json::Map<String, Value>`; `fn new(map) -> Self`, `fn get(&self, name: &str) -> Option<&Value>`; `Debug` prints `RawTokenResponse([redacted])`).
  - `TokenExchange { tokens: OAuthTokens, raw: RawTokenResponse }`.
  - `AccountToken { access_token: SecretString, expires_at: Option<DateTime<Utc>> }` (`Clone`, redacting `Debug`).
  - `GrantExchange { tokens: OAuthTokens, remote_user_id: Option<String>, reconnect_by: Option<DateTime<Utc>>, account_tokens: BTreeMap<String, AccountToken> }`, `GrantExchange::from_tokens(OAuthTokens) -> Self`.
  - `Agent::after_exchange(&self, ctx: &ExchangeContext, exchange: TokenExchange) -> Result<GrantExchange, AgentError>`; default `Ok(GrantExchange::from_tokens(exchange.tokens))`.
  - `EventOutcome { response: EventResponse, deauthorized: Vec<String> }`, `EventOutcome::respond(EventResponse) -> Self`. `Agent::handle_event` now returns `Result<EventOutcome, AgentError>`. `EventContext` gains `http: HttpHandles`.
  - `CredentialStore::refresh(&self, stale: &OAuthTokens) -> Result<OAuthTokens, AgentError>` (default `Err(AgentError::Authentication { message: "this credential cannot be refreshed; reconnect the account" })`: a store that cannot refresh leaves the credential unusable, and no `Capability` variant fits an `Unsupported`), `CredentialStore::account_token(&self, account: &AccountRef) -> Result<Option<AccountToken>, AgentError>` (default `Ok(None)`).
  - All new types re-exported from `postit_core`.

- [ ] **Step 1: Write failing tests**: `spec_defaults_client_id_param` (`new()` gives `"client_id"` and empty `extra_token_params`; builders set them); `after_exchange_default_passes_tokens_through` (fake agent; result has the same `expires_at`, no `remote_user_id`, empty `account_tokens`); `debug_redacts_exchange_types` (`format!("{:?}")` of `RawTokenResponse` with `{"access_token":"tok-1"}`, of `AccountToken`, and of `GrantExchange` contains no `tok-1`); `credential_store_defaults` (`account_token` is `Ok(None)`, `refresh` is `Authentication`); `handle_event_default_is_unsupported` updated for `EventOutcome`.
- [ ] **Step 2: Run** `cargo test -p postit-core` — FAIL (types missing).
- [ ] **Step 3: Implement** the types and defaults; fix every compile error across the workspace (`cargo check --workspace --all-targets --all-features`).
- [ ] **Step 4: Run** `cargo test -p postit-core -p postit-agent-kit -p postit-agent-mock` — PASS.
- [ ] **Step 5: Commit** `feat(core): add the OAuth exchange hook, event outcomes, and token refresh to the agent contract`.

### Task 2: Mock OAuth provider and conformance checks (`postit-agent-kit`)

**Files:**
- Modify: `backend/crates/agent-kit/src/testkit/oauth_provider.rs`, `backend/crates/agent-kit/src/testkit/fakes.rs` (in-memory `CredentialStore` gains `refresh` and `account_token`), `backend/crates/agent-kit/src/testkit/conformance/checks.rs`, `.../conformance/mod.rs`
- Test: `backend/crates/agent-kit/tests/testkit.rs`, `backend/crates/agent-kit/tests/conformance_http.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces on `MockOAuthProvider`:
  - `fn set_client(&self, client_id: &str, client_secret: &str)`: the token and revoke endpoints then require that client (Basic or body, per the request) and answer 401 `invalid_client` otherwise. Unset: any client accepted (existing tests keep working).
  - `fn approve(&self, authorize_url: &Url) -> String`: reads `state`, `code_challenge`, `code_challenge_method`, `redirect_uri` from the URL, stores a one-time code bound to them, returns the code. The token endpoint then checks `code_verifier` (S256 of it must equal the challenge), the same `redirect_uri`, and single use; a mismatch answers 400 `invalid_grant`. Codes not issued by `approve` keep the old permissive behavior.
  - `fn set_granted_scopes(&self, scopes: Option<Vec<String>>)`: when `Some`, token responses carry `"scope": "<space-joined>"`.
  - `fn set_refresh_issued(&self, issued: bool)` (default true): when false, responses omit `refresh_token`.
  - `fn token_requests(&self) -> usize` (count of `/token` requests only).
  - `fn set_delay(&self, delay: Option<Duration>)`: the token endpoint answers after `delay` (wiremock `set_delay`), for the callback deadline test.
  - `set_client` reads the client ID from HTTP Basic or the body's `client_id`; a spec with another `client_id_param` is tested against its own wiremock server (Task 6), not through `set_client`.
- In-memory credential store fake: `refresh` returns the next scripted `OAuthTokens` and counts calls (`refresh_calls()`); `account_token` returns from a map set by `with_account_token(remote_id, AccountToken)`.
- Conformance: for an agent whose `connect` is `OAuth`, `config_schema()` must exist and declare `client_id` (string) and `client_secret` (string, secret). Violation name `oauth_config_fields`.

- [ ] **Step 1: Write failing tests**: `approved_code_requires_matching_verifier` (approve, exchange with the right verifier succeeds, with a wrong one 400 `invalid_grant`), `approved_code_is_single_use`, `approved_code_requires_same_redirect_uri`, `wrong_client_is_rejected` (after `set_client`), `granted_scopes_are_reported`, `refresh_token_can_be_withheld`, `delay_postpones_the_token_answer`, `conformance_flags_oauth_agent_without_client_fields` (echo agent with an OAuth connect and no config schema fails with `oauth_config_fields`).
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit --features testkit` — FAIL.
- [ ] **Step 3: Implement.** PKCE S256 check: `BASE64URL_NOPAD(SHA256(verifier)) == challenge`.
- [ ] **Step 4: Run** the same — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): verify PKCE, redirect URI, and client in the mock OAuth provider`.

### Task 3: `mock_oauth` hooks (`postit-agent-mock`)

**Files:**
- Modify: `backend/crates/agents/mock/src/lib.rs`, `src/descriptor.rs`, `src/script.rs`, `src/client.rs` (discovery count for OAuth), new `src/oauth.rs`, `backend/crates/agents/mock/Cargo.toml` (`hmac`, `sha2`, `hex` from `[workspace.dependencies]`)
- Test: `backend/crates/agents/mock/tests/oauth.rs` (create), `tests/conformance.rs` (run `mock_oauth` through conformance with its config)

**Interfaces:**
- Consumes: Task 1, Task 2.
- Produces:
  - `mock_oauth` `config_schema()`: `client_id` (string, required), `client_secret` (string, required, `x-postit-secret`).
  - `MockScript::set_exchange(ExchangeScript)` where `ExchangeScript { remote_user_id: Option<String>, reconnect_by: Option<DateTime<Utc>>, account_tokens: Vec<String> /* remote ids that get "acct-token-<id>" */, fail: bool }`. Default (unset): `remote_user_id = Some("mock-user")`, nothing else. `fail: true` returns `AgentError::Authentication`. `MockScript::exchange_calls() -> usize` counts `after_exchange` calls.
  - `MockScript::set_oauth_accounts(n: usize)`: OAuth discovery returns `n` accounts with remote IDs `oauth-1` … `oauth-n` (default 1, remote ID `oauth`, matching today).
  - `handle_event` for kind `"deauthorize"`: body is JSON `{"user_id": "<remote>"}`; header `X-Mock-Signature` is lowercase hex HMAC-SHA256 of the raw body keyed with `client_secret` from `ctx.config`, checked by hex-decoding the header and calling `Mac::verify_slice` (constant time; never compare hex strings). Valid: `EventOutcome { response: 200 empty, deauthorized: [user_id] }`. Bad or missing signature: `AgentError::Authentication`. Any other kind: `Unsupported`.
  - Module docs in `lib.rs` state that an OAuth agent should return `remote_user_id` from `after_exchange` (spec, "A grant with no remote_user_id").

- [ ] **Step 1: Write failing tests**: `exchange_defaults_to_mock_user`, `exchange_script_sets_tokens_and_reconnect_by`, `exchange_failure_is_authentication`, `oauth_discovery_returns_scripted_count`, `deauthorize_with_valid_signature_names_the_user`, `deauthorize_with_bad_signature_is_refused`, `unknown_event_kind_is_unsupported`, and `mock_oauth` passing conformance with `{"client_id":"c","client_secret":"s"}`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-mock --features testkit` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(agent-mock): script the OAuth exchange, discovery, and deauthorize events`.

### Task 4: Schema and data layer

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql` (`account_grants.credential` nullable; `oauth_states` gains `consumed_at TIMESTAMPTZ` and `remote_user_id TEXT` (in clear, as on `account_grants`, for the revoke-skip rule); new indexes `account_grants_remote_user_idx ON account_grants (platform_id, remote_user_id) WHERE remote_user_id IS NOT NULL`, `oauth_states_remote_user_idx ON oauth_states (platform_id, remote_user_id) WHERE remote_user_id IS NOT NULL`, and `account_grants_refresh_idx ON account_grants (expires_at) WHERE kind = 'oauth' AND status = 'active'`; the `reconnect_by`, `oauth_states.expires_at`, and `pending_connections.expires_at` indexes already exist), `backend/crates/data/src/accounts.rs`, `backend/crates/data/src/audit.rs` (`AccountRevokedByPlatform` / `account_revoked_by_platform`), `backend/crates/data/src/lib.rs`, `backend/crates/data/src/advisory_lock.rs`
- Create: `backend/crates/data/src/oauth_states.rs`
- Test: `backend/crates/data/tests/oauth_states.rs` (create), `backend/crates/data/tests/accounts.rs` (extend), existing `tests/schema.rs` and reseal tests must stay green

**Interfaces:**
- Produces in `postit_data::oauth_states`:
  - `NewOAuthState { id: OAuthStateId, state_hash: [u8; 32], user: UserId, platform: PlatformId, pkce_verifier: Option<Sealed<Value>>, return_to: String, expires_at: DateTime<Utc> }`.
  - `OAuthState { id, user, platform, pkce_verifier, return_to, expires_at, connect_id_hash: Option<Vec<u8>>, grant_tokens: Option<Sealed<Value>>, discovered: Option<Value>, remote_user_id: Option<String> }`.
  - `OAuthStateRepo::seal_context(id: OAuthStateId, column: &'static str) -> SealContext`.
  - `insert(conn, NewOAuthState) -> Result<(), DataError>`.
  - `consume_state(conn, state_hash: &[u8; 32], now) -> Result<Option<OAuthState>, DataError>`: one `UPDATE … SET consumed_at = now WHERE state_hash = $1 AND consumed_at IS NULL RETURNING …`; returns the row even when expired (the service decides).
  - `store_result(conn, id, connect_id_hash: &[u8; 32], grant_tokens: &Sealed<Value>, discovered: &Value, remote_user_id: Option<&str>) -> Result<(), DataError>`.
  - `store_discarded(conn, id, grant_tokens: &Sealed<Value>, remote_user_id: Option<&str>, now) -> Result<(), DataError>`: a callback failure after the exchange; tokens kept, `connect_id_hash` stays NULL, `expires_at = now`.
  - `delete(conn, id) -> Result<(), DataError>` (rows holding no tokens only; the service never deletes a row with `grant_tokens`).
  - `find_result(conn, connect_id_hash) -> Result<Option<OAuthState>, DataError>` (no lock, no delete; pending read) and `lock_result(tx, connect_id_hash) -> Result<Option<OAuthState>, DataError>` (`SELECT … FOR UPDATE`; confirm decides between `delete` and `discard` in the same transaction).
  - `discard(conn, id, now) -> Result<(), DataError>`: `connect_id_hash = NULL`, `expires_at = LEAST(expires_at, now)`; tokens and `remote_user_id` kept.
  - `take_expired_result(conn, now) -> Result<Option<OAuthState>, DataError>`: `DELETE FROM oauth_states WHERE id = (SELECT id … WHERE expires_at <= $1 AND grant_tokens IS NOT NULL ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING …`. Each call removes the row it returns, so the purge loop ends, and two purges never revoke the same row.
  - `take_results_of(conn, user: UserId) -> Result<Vec<OAuthState>, DataError>`: `DELETE … WHERE user_id = $1 AND grant_tokens IS NOT NULL RETURNING …` (the deletion step).
  - `purge_expired(conn, now) -> Result<u64, DataError>`: deletes expired `oauth_states` holding no tokens and expired `pending_connections`; returns the total. The purge job calls it after draining `take_expired_result`.
  - `remote_user_in_use(conn, platform, remote_user_id, except: RemoteUserExcept, now) -> Result<bool, DataError>`, `enum RemoteUserExcept { Grant(GrantId), State(OAuthStateId), Nothing }`: an `active` grant of that platform and remote user (any owner), or an `oauth_states` row of it with `grant_tokens IS NOT NULL`, `connect_id_hash IS NOT NULL`, and `expires_at > now`, other than `except`. The revoke-skip rule (Global Constraints).
- `PendingConnectionRepo::find(conn, connect_id_hash) -> Result<Option<PendingConnection>, DataError>` (non-consuming; used by the pending read).
- `Grant.credential` becomes `Option<Sealed<Value>>`; `NewGrant.credential` stays required.
- System-side `GrantRepo` calls (no `OwnerScope`; the caller is a job or the engine, `updated_by` null):
  - `lock_for_refresh(tx, id: GrantId) -> Result<Option<(UserId, Grant)>, DataError>`: `xact_lock_pair(tx, GRANT_LOCK_NAMESPACE, key_of(id))`, then reads the grant and its owner.
  - `store_tokens(tx, id, credential: &Sealed<Value>, expires_at: Option<DateTime<Utc>>) -> Result<(), DataError>`.
  - `mark_needs_reconnect(tx, id) -> Result<Vec<(UserId, AccountId)>, DataError>` (grant and its non-disconnected accounts; returns the accounts for audit).
  - `due_for_refresh(conn, before: DateTime<Utc>, after: Option<GrantId>, limit: i64) -> Result<Vec<GrantId>, DataError>`: active `oauth` grants with `expires_at < before` and `id > after`, `ORDER BY id`. Keyset paging, so a grant that stays due is not returned again in the same run. SQL cannot tell whether a grant holds a refresh token (it is sealed); the caller decides after opening it.
  - `reconnect_by_passed(conn, now) -> Result<Vec<GrantId>, DataError>` and `set_reconnect_warned(tx, id, "expired")`.
  - `revoke_by_remote_user(tx, platform, remote_user_id) -> Result<Vec<(UserId, AccountId)>, DataError>`: grants become `revoked`, `credential = NULL`, their `account_tokens` deleted, non-disconnected accounts `revoked`.
  - `unrevoked_oauth_grants_of(conn, owner: UserId) -> Result<Vec<Grant>, DataError>` (`status <> 'revoked'`: a `needs_reconnect` grant past its `reconnect_by` may still be live at the provider) and `set_revoked(tx, id)` (status `revoked` and `credential = NULL`, like `revoke_by_remote_user`).
- `AccountTokenRepo::delete_for_accounts(tx, scope, accounts: &[AccountId]) -> Result<u64, DataError>`.
- `advisory_lock::xact_lock_pair(conn, namespace: i32, key: i32)` using `pg_advisory_xact_lock(int4, int4)`; `GRANT_LOCK_NAMESPACE: i32 = 1_007`; `key_of(GrantId) -> i32` = first 4 bytes of the UUID's SHA-256, big-endian. (Two-int locks do not share the bigint key space of the existing fixed keys.)

- [ ] **Step 1: Write failing tests** (`#[sqlx::test]`): `state_consumed_once` (second `consume_state` returns `None`), `expired_state_is_returned_for_the_service_to_refuse`, `result_found_without_consuming_then_locked`, `discard_hides_the_result_and_keeps_the_tokens` (`find_result` and `lock_result` then return `None`; the row is expired with `grant_tokens` and `remote_user_id` intact), `take_expired_result_returns_each_row_once` (two expired rows with tokens, one live: two calls return them, the third `None`, the live row stays), `take_results_of_takes_only_that_users_rows`, `purge_removes_only_expired_tokenless_states_and_pending_connections` (an expired row with tokens survives it), `revoke_by_remote_user_clears_credentials_and_tokens` (two grants of the user under two owners, one of another user untouched), `mark_needs_reconnect_skips_disconnected_accounts`, `due_for_refresh_selects_active_oauth_within_window`, `due_for_refresh_pages_by_id` (three due grants, `limit` 2: the second page starts after the first page's last ID and the third call is empty), `set_revoked_clears_the_credential`, `unrevoked_oauth_grants_include_needs_reconnect`, `delete_for_accounts_removes_only_those_tokens`, `remote_user_in_use_counts_active_grants_and_live_results` (true for another owner's active grant and for a live undiscarded result; false for the excepted grant or row, a revoked or `needs_reconnect` grant, an expired or discarded result, another platform), `grant_lock_serializes_two_transactions` (second `lock_for_refresh` blocks until the first commits, checked with a timeout), `audit_kind_account_revoked_by_platform_round_trips`. Existing `tests/schema.rs` still passes with the nullable column.
- [ ] **Step 2: Run** `cargo test -p postit-data -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement**, then `cargo sqlx prepare` from `backend/crates/data`. Update every `Grant.credential` reader (`postit-oauth/src/store.rs`: `None` means reconnect).
- [ ] **Step 4: Run** `cargo test -p postit-data -p postit-oauth -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(data): add OAuth states, grant refresh locking, and platform revocation`.

### Task 5: Settings

**Files:**
- Modify: `backend/crates/config/src/settings_publishing.rs` (drop "Reserved" from `OAuthSettings`), `backend/crates/config/src/settings.rs` (`JobSchedules.refresh_credentials`, `JobSchedules.oauth_state_purge`), `backend/config/default.toml`, `backend/crates/config/src/loader.rs` (test TOML), `docs/configuration.md`
- Test: `backend/crates/config/` tests

**Interfaces:**
- Produces: `JobSchedules.refresh_credentials: String` (default `0 */5 * * * *`), `JobSchedules.oauth_state_purge: String` (default `0 0 * * * *`). Validation: each `return_to_allowlist` entry parses as an absolute URL without query or fragment (error names `oauth.return_to_allowlist[<i>]`).

- [ ] **Step 1: Write failing tests**: `oauth_schedules_default` and `return_to_allowlist_rejects_query_and_fragment`.
- [ ] **Step 2: Run** `cargo test -p postit-config` — FAIL.
- [ ] **Step 3: Implement** with the `docs/configuration.md` rows (`POSTIT__JOBS__SCHEDULES__REFRESH_CREDENTIALS`, `…__OAUTH_STATE_PURGE`, `POSTIT__OAUTH__RETURN_TO_ALLOWLIST`) and a "Callback URL" note giving the redirect URI format.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(config): add the OAuth job schedules and validate the return_to allowlist`.

### Task 6: OAuth engine (`postit-oauth/src/engine.rs`)

**Files:**
- Create: `backend/crates/oauth/src/engine.rs`
- Modify: `backend/crates/oauth/src/lib.rs`, `backend/crates/oauth/src/service.rs` (`AccountServiceDeps` gains `public_url: Url` for the redirect URI; the engine downcasts the existing `http.default`, see Global Constraints), `backend/crates/oauth/src/testkit.rs`, `backend/crates/server/src/compose.rs` (pass them)
- Test: `backend/crates/oauth/tests/engine.rs` (create)

**Interfaces:**
- Consumes: Task 1 spec fields; Task 2 provider.
- Produces (crate-private unless noted):
  - `Pkce { verifier: SecretString, challenge: String }`, `Pkce::generate() -> Pkce` (32 random bytes, base64url no pad; challenge = base64url(SHA-256(verifier))).
  - `ClientCredentials { id: String, secret: SecretString }`, `ClientCredentials::from_config(&Value) -> Result<Self, AccountError>` (`PlatformUnavailable` when missing).
  - `authorize_url(spec, client: &ClientCredentials, redirect_uri: &Url, state: &str, pkce: Option<&Pkce>) -> Url`: `response_type=code`, `client_id_param`, `redirect_uri`, `scope` joined by `scope_separator`, `state`, `code_challenge` + `code_challenge_method=S256` when PKCE, then `extra_authorize_params`.
  - `async fn exchange_code(http, spec, client, redirect_uri, code: &SecretString, verifier: Option<&SecretString>, now) -> Result<TokenExchange, TokenError>`.
  - `async fn refresh(http, spec, client, refresh_token: &SecretString, now) -> Result<TokenExchange, TokenError>`: a response without `refresh_token` keeps none (the caller keeps the old one).
  - `async fn revoke(http, spec, client, token: &SecretString, hint: &str) -> Result<(), TokenError>`.
  - `enum TokenError { InvalidGrant, InvalidClient, Provider(String /* OAuth error code only */), Transport }` with `retryable() -> bool` (`Transport` and 5xx `Provider`).
  - `fn missing_scopes(spec, granted: &[String]) -> Vec<String>`: empty when `granted` is empty (response had no `scope`).
  - `expires_at = now + expires_in`, else `now + spec.default_access_ttl`, else `None`. `scopes` from the response's `scope` split on spaces or the spec separator.
  - Client auth: `Basic` → HTTP Basic; `PostBody` → `client_id_param` and `client_secret` in the form; `None` → `client_id_param` only. `extra_token_params` appended to code and refresh requests.
  - Idempotency per call as in Global Constraints; `postit_http`'s outcome-unknown error becomes `TokenError::Transport`.
  - `fn redirect_uri(public_url: &Url, platform: &PlatformId) -> Url` through `Url::join` (Global Constraints).

- [ ] **Step 1: Write failing tests** against `MockOAuthProvider` with `set_client`: `authorize_url_carries_pkce_state_scopes_and_extras`, `exchange_with_pkce_succeeds_and_parses_expiry`, `exchange_with_wrong_verifier_is_invalid_grant`, `post_body_and_basic_client_auth_both_work`, `custom_client_id_param_is_sent` (its own wiremock server with a body matcher on `client_key=`, not `MockOAuthProvider::set_client`), `refresh_without_rotation_returns_no_refresh_token`, `refresh_invalid_grant_is_classified`, `server_error_is_retryable`, `code_exchange_is_not_resent_after_a_5xx` (one queued `ServerError`: the exchange fails with one token request), `refresh_resend_follows_rotation` (one queued `ServerError`: a non-rotating spec retries and succeeds with two token requests; a rotating spec fails with one), `redirect_uri_has_no_double_slash` (`server.public_url` with and without a trailing slash), `revoke_sends_token_and_hint` (provider `revoked()` has the hash), `missing_scopes_only_when_response_lists_scopes`, `token_error_display_has_no_body` (provider body contains a token-like string; `format!("{err:?}")` does not).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test engine` — FAIL.
- [ ] **Step 3: Implement** with form-encoded POSTs through the `postit-http` client. Parse JSON into `RawTokenResponse`; never log it.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): add the provider-neutral OAuth token client`.

### Task 7: Start, callback, and pending read

**Files:**
- Create: `backend/crates/oauth/src/flow.rs`, `backend/crates/oauth/src/return_to.rs`
- Modify: `backend/crates/oauth/src/lib.rs`, `src/error.rs` (`AccountError::ReturnToInvalid`), `src/service.rs` (`AccountServiceDeps.return_to: ReturnToAllowlist`, `AccountServiceDeps.callback_deadline: Duration`, `pub const CALLBACK_DEADLINE: Duration` = 60 s), `src/connect.rs` (share `ConnectId` and `choose`), `src/testkit.rs` and `backend/crates/server/src/compose.rs` (fill the new fields so the workspace compiles; Task 12 only adds the jobs and the deletion step)
- Test: `backend/crates/oauth/tests/oauth_flow.rs` (create), `tests/common/mod.rs` (OAuth fixtures: provider, `mock_oauth` registered with `{client_id, client_secret}`)

**Interfaces:**
- Consumes: Tasks 3, 4, 6.
- Produces (public):
  - `ReturnToAllowlist::new(app_public_url: &Url, environment: Environment, extra: &[String]) -> Self`, `fn allows(&self, candidate: &str) -> bool` (exact string equality against the entries built as Global Constraints says: `Url::join` for the web entry, serialized `Url::parse` for the configured ones). `Environment` is `postit_config`'s environment enum. Task 5's validation guarantees every configured entry parses.
  - `AccountService::start_oauth(&self, scope: &OwnerScope, platform: &PlatformId, return_to: &str) -> Result<OAuthStart, AccountError>`; `OAuthStart { authorize_url: Url, expires_at: DateTime<Utc> }`. Errors: `Scope` (owner only), catalog errors, `ConnectMethodUnsupported`, `ReturnToInvalid`.
  - `AccountService::oauth_callback(&self, platform: &str, params: CallbackParams) -> Url` (never fails; returns the 303 target). `CallbackParams { code: Option<SecretString>, state: Option<String>, error: Option<String> }`.
  - `enum CallbackCode { AccessDenied, ProviderError, StateInvalid, ExchangeFailed, ScopeMissing, DiscoveryFailed, NoAccounts, PlatformUnavailable }` with `as_str()` giving the Global Constraints strings.
  - `AccountService::pending(&self, scope: &OwnerScope, connect_id: &str) -> Result<PendingView, AccountError>`; `PendingView { platform: PlatformId, connect: ConnectKind, expires_at: DateTime<Utc>, discovered: Vec<DiscoveredAccount> }`. Looks in `pending_connections` then `oauth_states`; a foreign owner's read ends the result and returns `NotFound`: `PendingConnectionRepo::take` for a credential result (it holds no OAuth tokens), `OAuthStateRepo::discard` for an OAuth result (the purge revokes it); expired or unknown → `NotFound`.
- Callback order is the spec's steps 1–6. A failure before or at the exchange (`state` problems, provider errors, `platform_unavailable` checked before the exchange, `exchange_failed`, a deadline during the exchange) deletes the row: no tokens exist. A failure after a successful exchange (`scope_missing`, `discovery_failed`, `no_accounts`, a deadline during `after_exchange` or discovery) seals what it has into `StoredExchange` and calls `store_discarded`, so the purge revokes it. Success calls `store_result` with `remote_user_id` from `after_exchange`. The stored `grant_tokens` is the sealed JSON of an internal `StoredExchange { access_token, refresh_token, expires_at, scopes, remote_user_id, reconnect_by, account_tokens: {remote_id: {access_token, expires_at}} }`, sealed with `OAuthStateRepo::seal_context(id, "grant_tokens")`. Discovery uses `after_exchange`'s tokens through an in-memory `CredentialStore` (`Credentials::OAuth`). No pooled connection is held across a provider call. Steps 3–5 run under `callback_deadline` (Global Constraints).

- [ ] **Step 1: Write failing tests** (`#[sqlx::test]`, provider + `mock_oauth`):
  - `start_returns_authorize_url_and_stores_hashed_state` (the URL's `state` is not stored in clear; `pkce_verifier` sealed).
  - `start_refuses_delegate_and_credentials_platform_and_unconfigured_platform`.
  - `return_to_allowlist_accepts_exact_entries` (web, `postit-dev://oauth-done` in development, an extra entry, an extra entry configured as `https://Example.com/done` accepting `https://example.com/done`; `app.public_url` with and without a trailing slash both give `…/oauth-done`, never `//oauth-done`) and `return_to_near_misses_are_rejected` (`…/oauth-done/`, `…/oauth-done?x=1`, `…/oauth-done#a`, `HTTPS://POSTIT.LOCAL:44315/oauth-done`, `%2Foauth-done`, `postit://oauth-done` in development, another host) — all `ReturnToInvalid`.
  - `callback_success_redirects_with_connect_id` (target is `return_to?result=ok&connect_id=<43 chars>`; row has `connect_id_hash`, sealed `grant_tokens`, `discovered`; no grant or account rows exist).
  - `callback_unknown_expired_and_wrong_platform_state_redirect_state_invalid` to `{app.public_url}/oauth-done?result=error&code=state_invalid`.
  - `callback_replay_does_not_touch_the_pending_result`.
  - `callback_with_error_and_code_never_exchanges` (`error=access_denied` → `access_denied`; `error=temporarily_unavailable` → `provider_error`; provider saw no token request; row deleted).
  - `callback_exchange_failure`, `callback_platform_switched_off_mid_flow`, `callback_past_deadline_redirects` (`callback_deadline` 200 ms, `provider.set_delay(Some(1 s))` → `exchange_failed`) — each redirects with its code to `return_to` and leaves no row.
  - `callback_scope_missing` (`set_granted_scopes(Some(["read"]))`), `callback_discovery_failure` (`ExchangeScript { fail: true }` → `discovery_failed`), `callback_no_accounts` (`set_oauth_accounts(0)`) — each redirects with its code to `return_to` and leaves only a discarded row: no `connect_id_hash`, expired, sealed `grant_tokens` and `remote_user_id` kept; `provider.revoked()` still empty (the purge revokes, Task 9).
  - `pending_read_by_owner_does_not_consume`, `pending_read_by_other_user_discards_and_404s` (the owner's read afterwards is `NotFound` too; the row is discarded with its tokens), `pending_read_works_for_credential_connections` (a foreign read there deletes the row, as B3's confirm does).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test oauth_flow -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): start OAuth connections, handle the callback, and read pending results`.

### Task 8: Confirm OAuth results

**Files:**
- Modify: `backend/crates/oauth/src/connect.rs` (`confirm` looks in both tables; `ConnectInput` gains `kind: GrantKind`, `remote_user_id`, `expires_at`, `reconnect_by`, `scopes`, `account_tokens: BTreeMap<String, AccountToken>`), `backend/crates/oauth/src/manage.rs` (`ConnectKind::OAuth` if missing)
- Test: `backend/crates/oauth/tests/oauth_confirm.rs` (create)

**Interfaces:**
- Consumes: Task 4 (`lock_result`, `discard`, `delete`), Task 7 (`StoredExchange`).
- Flow for an OAuth result, in one transaction: `lock_result`; a foreign caller or an expired result → `discard`, commit, `NotFound`; otherwise `connect_in_tx`; when it connected nothing (no account chosen, or every chosen account connected elsewhere) → `discard` (the new grant is already gone through `delete_if_unused`; the purge revokes the tokens under the skip rule, which spares them when another owner's active grant holds the same remote user), else `delete`. Commit. No provider call on this path.
- Produces: `Confirmed.connect == ConnectKind::OAuth` for OAuth results; grant row `kind = 'oauth'` with `remote_user_id`, `expires_at`, `reconnect_by`, `scopes`; accounts carry the grant's scopes; `account_tokens` rows for chosen accounts that have one (sealed with `AccountTokenRepo::seal_context`). `account_tokens` is keyed by account with no grant link, so the transaction first runs `AccountTokenRepo::delete_for_accounts` over every connected (created, reactivated, or moved) account, then inserts: a moved account never keeps its previous grant's token. Moving an account deletes its previous unused grant and never revokes it.

- [ ] **Step 1: Write failing tests**:
  - `confirm_creates_oauth_grant_and_chosen_accounts_only` (3 discovered, 2 chosen).
  - `several_accounts_share_one_grant_token` (both accounts' `PgCredentialStore::credentials()` return the same access token).
  - `confirm_by_other_user_is_404_and_discards` (the starter's confirm afterwards is `NotFound` too; the row is discarded with its tokens).
  - `expired_result_creates_nothing` (and is discarded, not deleted).
  - `confirm_with_nothing_chosen_discards` and `confirm_with_every_account_elsewhere_discards` (no grant row remains; the state row is discarded with its tokens).
  - `attacker_initiated_connection_attaches_nothing` (Review Focus 8): attacker starts; the victim's browser completes the callback; the attacker confirms with a guessed or absent `connect_id` → `NotFound`; the victim confirms the real `connect_id` → `NotFound` (not the starter) and the result is discarded; no grant or account exists for either user. (Task 9's `attacker_result_is_revoked_by_the_purge` continues this scenario through the purge.)
  - `account_connected_elsewhere_is_skipped_and_counted` (another owner holds `oauth-2`).
  - `reconnect_moves_account_and_deletes_old_grant_without_revoke` (provider `revoked()` stays empty).
  - `account_tokens_stored_for_chosen_accounts` (`ExchangeScript { account_tokens: ["oauth-1"] }`; `account_token(&oauth-1)` returns `acct-token-oauth-1`, `oauth-2` none).
  - `reconnect_drops_stale_account_token` (first connect gives `oauth-1` an account token; a reconnect whose exchange has none leaves `account_token(&oauth-1)` as `None`).
  - `credential_connect_never_calls_after_exchange` (B3's credential connect and confirm on `mock`: `exchange_calls() == 0`; the spec's conformance item, tested here because conformance cannot observe the engine).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test oauth_confirm -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement**; keep B3's `tests/connect.rs` green.
- [ ] **Step 4: Run** `cargo test -p postit-oauth -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): confirm OAuth connections into grants, accounts, and account tokens`.

### Task 9: Refresh and the credential store

**Files:**
- Create: `backend/crates/oauth/src/refresh.rs`, `backend/crates/oauth/src/jobs.rs`, `backend/crates/oauth/src/revoke.rs` (`revoke_tokens`; Task 10 adds `revoke_grant` beside it), `backend/crates/oauth/tests/purge.rs` (the purge tests)
- Modify: `backend/crates/oauth/src/store.rs` (OAuth arms), `backend/crates/oauth/Cargo.toml` (`postit-jobs`), `backend/crates/oauth/src/lib.rs`, every `PgCredentialStore::new` caller (`src/service.rs::agent_context`, `tests/connect.rs`, `tests/store.rs`)
- Test: `backend/crates/oauth/tests/refresh.rs` (create), `tests/store.rs` (extend)

**Interfaces:**
- Consumes: Task 4 `GrantRepo::lock_for_refresh`, `store_tokens`, `mark_needs_reconnect`, `due_for_refresh`, `reconnect_by_passed`, `OAuthStateRepo::take_expired_result`, `purge_expired`, `remote_user_in_use`; Task 6 `engine::refresh`, `engine::revoke`.
- Produces:
  - `AccountService::revoke_tokens(&self, platform: &PlatformId, config: &Value, tokens: &OAuthTokens)` (crate-private, best effort, no error): `spec.revoke_url` set → `engine::revoke` (the refresh token with hint `refresh_token`, else the access token with hint `access_token`); otherwise the agent's `client.revoke()` over an in-memory `CredentialStore` holding `tokens`. Failures are logged by error kind only. The one provider-revoke primitive: Task 10's `revoke_grant` and the deletion step call it too. The skip rule is the caller's job.
  - `AccountService::refresh_grant(&self, grant: GrantId, stale: Option<&OAuthTokens>) -> Result<OAuthTokens, AgentError>`: the spec's four steps. Comparison "tokens differ from `stale`" is by `expires_at` and a SHA-256 of the access token, never by printing them. `stale = None` (cron) always refreshes when still due after the lock. A grant whose opened credential has no refresh token never reaches the token endpoint: while `expires_at > now` it returns the current tokens; once `expires_at <= now` it marks `needs_reconnect` (with the audit events) and returns `Authentication`. Both the cron and the on-demand path go through this, so they cannot disagree.
  - `PgCredentialStore` needs the service's engine pieces: construct it from `AccountService` (`PgCredentialStore::new(service: AccountService, owner, grant)` replacing the pool+vault constructor; update callers). `credentials()` returns `Credentials::OAuth` for `kind = oauth`, refreshing first when `expires_at <= now` (it drops its read connection before calling `refresh_grant`, which holds its own transaction across the token call); `refresh(stale)` calls `refresh_grant`; `store_refreshed(tokens)` writes under the grant lock; `account_token(account)` reads `account_tokens` by `(grant's owner, remote id)` and opens it.
  - `RefreshCredentials` job (`JOB_TYPE = "refresh_credentials"`, `Queue::Maintenance`) and `OAuthStatePurge` (`"oauth_state_purge"`, `Queue::Maintenance`); `postit_oauth::jobs::register(registry, service: AccountService, schedules: &JobSchedules) -> Result<(), JobsError>` and `postit_oauth::jobs::console(catalog, schedules)`.
  - The cron, `run_refresh_credentials(service, now, batch_size) -> RefreshReport` (the job passes `REFRESH_BATCH = 100`): pages `due_for_refresh(now + 15 min, after, batch_size)` by ID, advancing `after` to each page's last ID until a page comes back short, one `refresh_grant(id, None)` each (a grant with no refresh token is handled inside it, see above); a failure on one grant is logged by error kind and the run moves on; then `reconnect_by_passed(now)` → `mark_needs_reconnect` and `reconnect_warned = 'expired'`. Each `needs_reconnect` records `account_needs_reconnect` per account with no actor.
  - The purge, `run_oauth_state_purge(service, now) -> PurgeReport { revoked, skipped, purged }` (counts only): loops `take_expired_result(now)` until `None`; for each row, opens `grant_tokens` (`seal_context(id, "grant_tokens")`) into `StoredExchange`, then calls `revoke_tokens` unless `remote_user_id` is `None` or `remote_user_in_use(…, RemoteUserExcept::State(id), now)`, or the platform no longer resolves (no client credentials to revoke with; logged). A row that fails to open is counted and dropped. Each row is already deleted when its revoke runs, so a crash mid-run loses at most that one revoke and never repeats one. No pooled connection is held across a provider call. Then `purge_expired(now)`.

- [ ] **Step 1: Write failing tests**:
  - `cron_refreshes_grants_expiring_within_15_minutes_only`.
  - `on_demand_refresh_after_401` (store `refresh(stale)` returns new tokens; `credentials()` then returns them).
  - `rotation_is_stored` and `no_rotation_keeps_the_old_refresh_token`.
  - `invalid_grant_sets_needs_reconnect_with_audit` and `expired_refresh_token_sets_needs_reconnect` (`OAuthFailure::ExpiredRefreshToken`).
  - `server_error_leaves_grant_active`.
  - `expired_without_refresh_token_needs_reconnect` (Review Focus 3; both through `credentials()` and through the cron) and `unexpired_without_refresh_token_makes_no_request`.
  - `cron_does_not_loop_on_grants_that_stay_due` (Review Focus 7: three due grants on a rotating spec (so a 5xx is not resent), three queued `ServerError`s, `batch_size` 2; the run returns, `token_requests() == 3`, all three still `active`).
  - `concurrent_refresh_across_two_pools_makes_one_request`: two `PgPool`s on the same database, two services, `tokio::join!` of `refresh_grant(id, Some(&stale))`; `provider.token_requests() == 1` and both return the same new token.
  - `reconnect_by_passed_sets_needs_reconnect`.
  - `account_token_is_served_for_its_account_only`.
  - `purge_job_deletes_expired_rows`.
  - `purge_revokes_discarded_results` (rows discarded by a callback `no_accounts`, a nothing-chosen confirm, and a plain expiry: each refresh token hash in `provider.revoked()`, rows gone).
  - `purge_skips_results_whose_remote_user_is_in_use` (one discarded result whose `mock-user` has an active grant of another owner, one whose `mock-user` has another live pending result: no revoke, rows gone, the live result untouched).
  - `purge_skips_results_without_remote_user_id` and `purge_drops_results_of_an_unavailable_platform` (platform disabled: no revoke, row gone, the run finishes).
  - `purge_uses_the_agent_revoke_without_revoke_url` (spec without `revoke_url`: mock `revoke_calls() == 1`).
  - `attacker_result_is_revoked_by_the_purge` (Review Focus 8: Task 8's scenario, then the purge; `provider.revoked()` holds the victim's refresh token hash).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test refresh --test purge -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-oauth -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): refresh OAuth grants under a per-grant lock, revoke discarded results, and add the OAuth jobs`.

### Task 10: Revocation, platform events, and the deletion step

**Files:**
- Create: `backend/crates/oauth/src/events.rs`
- Modify: `backend/crates/oauth/src/revoke.rs` (from Task 9), `backend/crates/oauth/src/disconnect.rs` (`revoke_and_delete` → `revoke_grant`), `backend/crates/oauth/Cargo.toml` (`postit-identity`, for `OAuthDeletionStep`; plan 03 puts `identity` below the service crates, so no cycle), `backend/crates/identity/src/deletion.rs`, `backend/crates/identity/src/jobs.rs` (`IdentityJobs.deletion_steps`), `backend/crates/identity/src/lib.rs`, every `IdentityJobs { … }` literal (server compose and tests)
- Test: `backend/crates/oauth/tests/revoke.rs`, `backend/crates/oauth/tests/events.rs` (create), `backend/crates/identity/tests/deletion.rs` (extend)

**Interfaces:**
- Consumes: Task 4 `remote_user_in_use`, `revoke_by_remote_user`, `unrevoked_oauth_grants_of`, `set_revoked`, `lock_for_refresh`, `OAuthStateRepo::take_results_of`; Task 9 `revoke_tokens`.
- Produces:
  - `AccountService::revoke_grant(&self, owner: UserId, grant: &Grant)` (best effort, no error): OAuth with `remote_user_id = None` → no provider call; `remote_user_in_use(…, RemoteUserExcept::Grant(id), now)` → no call (an active grant of any owner, or a live pending result, of the same remote user); otherwise `revoke_tokens`. Credentials grants → `client.revoke()` as in B3. For an OAuth grant it takes `lock_for_refresh` and reads the credential inside that transaction, holding the lock across the revoke call, so a refresh running at the same moment finishes first and the revoke sends the rotated token, not the dead one.
  - `AccountService::handle_platform_event(&self, platform: &str, event: InboundEvent) -> Result<EventResponse, AccountError>`: `Unsupported` and every catalog error (unknown, disabled, or unconfigured platform) → `NotFound`, so the route does not reveal which platforms exist; agent `Authentication` → new `AccountError::EventRejected`; applies `revoke_by_remote_user` per name in one transaction with `account_revoked_by_platform` audit events (details `account_id`, `platform`); returns `outcome.response`.
  - In `postit-identity`: `#[async_trait] pub trait UserDeletionStep: Send + Sync { fn name(&self) -> &'static str; async fn run(&self, user: UserId) -> Result<(), JobError>; }`; `DeleteUserHandler::with_steps(self, Vec<Arc<dyn UserDeletionStep>>) -> Self`; steps run in order after step 1, before step 2, each logged by `name()`. `IdentityJobs.deletion_steps: Vec<Arc<dyn UserDeletionStep>>`.
  - `postit_oauth::OAuthDeletionStep::new(service: AccountService)` (`name() = "oauth"`): first `take_results_of(user)` and, for each, `revoke_tokens` under the skip rule (`RemoteUserExcept::Nothing`; the user's results are already gone, so what counts is any active grant, the user's own included, and other users' live results). Then, for each `unrevoked_oauth_grants_of(user)`: `revoke_grant`, then `set_revoked` (which clears the credential). Results go first, so a result that shares its remote user with one of the user's grants is spared and the grant's revoke ends that authorization once. Without this the user's row deletion would cascade the results away unrevoked.

- [ ] **Step 1: Write failing tests**:
  - `disconnecting_one_of_two_accounts_keeps_the_grant_unrevoked`, `disconnecting_the_last_account_revokes_at_revoke_url` (`provider.revoked()` has the refresh token hash).
  - `revoke_skipped_without_remote_user_id`, `revoke_skipped_when_another_owner_holds_the_remote_user` (Review Focus 4), `revoke_skipped_while_a_pending_result_holds_the_remote_user` (the same user has a live result from another tab), `agent_revoke_used_when_spec_has_no_revoke_url` (spec without `revoke_url`; mock `revoke_calls() == 1`).
  - `deauthorize_event_revokes_matching_grants`: two owners' grants of `mock-user` and one of `other-user`; valid signed event → first two `revoked`, `credential IS NULL`, accounts `revoked`, account tokens gone, audit events written; `other-user` untouched.
  - `deauthorize_with_bad_signature_changes_nothing` (`EventRejected`), `event_kind_unsupported_is_not_found`, `event_for_unconfigured_platform_is_not_found`.
  - `revoke_waits_for_a_running_refresh` (the test holds `lock_for_refresh` in an open transaction and stores rotated tokens; `revoke_grant` started meanwhile blocks, then revokes the rotated refresh token's hash after the commit).
  - Identity: `deletion_steps_run_in_order_and_resume` (two recording steps; `failing_after(2)`, so the steps ran before the injected failure; the retry runs both steps again, in order, and finishes). Checkpoint 1 fires before the steps, so `failing_after(1)` would not prove a rerun. In `postit-oauth`: `oauth_deletion_step_revokes_once` (run twice; provider sees one revoke), `oauth_deletion_step_revokes_needs_reconnect_grants`, and `oauth_deletion_step_revokes_pending_results` (a live result and a discarded one: both revoked, rows gone; a result sharing its remote user with the user's own grant is revoked once through the grant).
- [ ] **Step 2: Run** `cargo test -p postit-oauth -p postit-identity -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): revoke grants safely, apply platform deauthorizations, and add the OAuth deletion step`.

### Task 11: API routes

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs`, `backend/crates/api/src/routes/mod.rs` (the callback and event routes go in `v1_custom_limits_router` through `crate::router::with_limits`, the pattern `router.rs` documents for routes with their own limits), create `backend/crates/api/src/routes/platforms.rs`, `backend/crates/api/src/dto_accounts.rs`, `backend/crates/api/src/error.rs` (`ErrorCode::ReturnToInvalid` 422; `AccountError::ReturnToInvalid` → it; `EventRejected` → 401 with no detail), `backend/crates/api/src/openapi.rs`
- Test: `backend/crates/api/tests/oauth.rs` (create)

**Interfaces:**
- Consumes: Tasks 7, 8, 10.
- Produces:
  - `POST /api/v1/accounts/connect/{platform}` body `{ "return_to": string }` → 200 `{ "authorize_url": string, "expires_at": string }`. Scope extractor; owner only.
  - `GET /api/v1/accounts/connect/{connect_id}` → 200 `{ "platform", "connect": "oauth"|"credentials", "expires_at", "discovered": [DiscoveredAccount] }`. Owner only. axum's router (matchit 0.8) refuses two same-shape routes with different parameter names, so one route `/accounts/connect/{id}` serves GET and POST; each operation's utoipa `params` describes `id` (a platform ID for POST, a connect ID for GET) so the generated TypeScript and Dart clients say which. `POST /accounts/connect/{connect_id}/confirm` has another shape and keeps its name.
  - `GET /api/v1/platforms/{id}/oauth/callback` (query `code`, `state`, `error`, `error_description` ignored) → 303 `Location`. `Cache-Control: no-store` and `Referrer-Policy: no-referrer` come from the existing `api_headers` layer; the tests assert them. No bearer; not behind `Scope`. Timeout `CALLBACK_TIMEOUT` (Global Constraints) through `with_limits`.
  - `POST /api/v1/platforms/{id}/events/{kind}` raw body (constant `EVENT_BODY_LIMIT` = 64 KiB through `with_limits`) → the agent's status, `Content-Type`, and body; 404 `not_found` for unsupported or unavailable platforms; 401 for rejected.
  - OpenAPI: the callback operation's description lists the result codes from Global Constraints.

- [ ] **Step 1: Write failing tests** (server testkit with `mock_oauth` and the provider):
  - `full_flow_over_http`: start → `provider.approve(authorize_url)` → `GET callback` (303 to `return_to?result=ok&connect_id=…`, `no-store`, `no-referrer`) → `GET /accounts/connect/{id}` lists accounts → confirm → `GET /accounts` shows them.
  - Isolation, four callers on start, pending read, and confirm: owner OK; delegate at `view`, `edit`, `publish` → 403 `owner_only`; delegate limited to other accounts → 403 `owner_only`; unrelated user → 404 on pending read and confirm, and start creates a flow in their own workspace only.
  - `return_to_invalid_is_422`.
  - `callback_needs_no_bearer_and_ignores_act_as`.
  - `callback_outlives_the_default_request_timeout` (the testkit's API `request_timeout` 1 s, `callback_deadline` 2 s, `provider.set_delay(Some(1500 ms))`: 303 with `result=ok`, not 408).
  - `events_route_applies_deauthorization` and `events_route_rejects_bad_signature_with_401_and_empty_detail`, `events_route_body_over_limit_is_413`.
  - `oauth_redirects_and_problems_leak_nothing`: run the flow through the shared no-secret-leak harness (`crates/oauth/tests/leak.rs` pattern / P10 harness) with the provider's tokens, the client secret, the PKCE verifier, and the `state` as needles; only the success redirect may contain the `connect_id`.
- [ ] **Step 2: Run** `cargo test -p postit-api --test oauth -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-api -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(api): add the OAuth connect, callback, pending, and platform event routes`.

### Task 12: Server wiring

**Files:**
- Modify: `backend/crates/server/src/compose.rs` (`AccountServiceDeps` new fields, `ReturnToAllowlist::new(&settings.app.public_url, env, &settings.oauth.return_to_allowlist)` with the `Environment` the server loaded its settings for, `postit_oauth::jobs::register` and `console`, `IdentityJobs.deletion_steps = vec![Arc::new(OAuthDeletionStep::new(accounts.clone()))]`), `backend/crates/server/src/platforms.rs` if the event/callback context needs it
- Test: `backend/crates/server/tests/` (extend the existing job-registration test), `backend/crates/server/tests/oauth.rs` (create, worker role)

**Interfaces:**
- Consumes: Tasks 5, 9, 10.
- Produces: a worker registers `refresh_credentials` and `oauth_state_purge` (visible in `GET /admin/jobs/recurring`); `delete_user` runs the OAuth step.

- [ ] **Step 1: Write failing tests**: `worker_registers_oauth_recurring_jobs`, `delete_user_revokes_oauth_grants` (user with a `mock_oauth` grant and, for another remote user, a pending result → delete → provider saw two revokes and the user's rows are gone), `worker_purge_revokes_a_discarded_result` (trigger `oauth_state_purge` through the admin console on a discarded row; provider saw the revoke).
- [ ] **Step 2: Run** `cargo test -p postit-server -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(server): register the OAuth jobs, return_to allowlist, and deletion step`.

### Task 13: Contract, clients' error maps, docs, deferred file, and gates

**Files:**
- Regenerate: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/` (`cargo xtask openapi`, from PowerShell; fix any `mobile/lib/state` breakage from renamed inline types)
- Modify: `web/src/i18n/en-US/common.json`, `web/src/i18n/ar/common.json` (`return_to_invalid`), `web/src/features/audit/kinds.ts` and `web/src/i18n/{en-US,ar}/audit.json` (`account_revoked_by_platform`), `mobile/lib/l10n/app_en.arb`, `app_ar.arb`, `mobile/lib/ui/errors/messages.dart` (`return_to_invalid`)
- Modify docs: `docs/deployment.md` (registering a platform developer app: the redirect URI; dev tunnel fallback with cloudflared or ngrok as `server.public_url`; a per-provider redirect-URI rules table for TikTok, X, YouTube/Google, LinkedIn, Meta with "verify at the F phase start" and a link to each provider's doc), `docs/security.md` (confirm-step rationale; the revoke-skip rule, including live pending results; grants and results without `remote_user_id`; discarded results: which paths discard, that the hourly purge is the one place that revokes them, that their sealed tokens sit in the database and its backups until then), `CLAUDE.md` (project state: B7 built, one sentence like B3's)
- Create: `docs/superpowers/plans/2026-10-28-b7-oauth-connection-deferred.md`

- [ ] **Step 1: Run** `cargo xtask openapi` then `cargo xtask openapi --check` — PASS; web `pnpm` type check and lint per `web/CLAUDE.md`; in `mobile/`, `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test` — PASS.
- [ ] **Step 2: Write** the docs and the deferred file. Deferred entry: *Owner notifications for `needs_reconnect`, `account_revoked_by_platform`, and the `reconnect_by` warnings at 7 days and 1 day (`reconnect_warned` stages `7d`, `1d`). Why not now: they need `postit-notify`, which D1 creates. Trigger: D1 (its notification kinds and the `refresh_credentials` cron gain the warning stages).* Add any other item that truly cannot be fixed in B7, with its trigger; fix everything else.
- [ ] **Step 3: Run every gate** from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3` (background; 30+ minutes) — all PASS.
- [ ] **Step 4: Commit** `docs: describe OAuth connections and record what waits for D1` (contract regeneration may be its own `chore(api): regenerate the contract` commit).
