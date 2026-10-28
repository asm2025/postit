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
- Token, refresh, and revoke calls go through `HttpHandles::default` (downcast as `postit-agent-kit::http` does, or pass the concrete `postit_http` client into the engine from `AccountServiceDeps`). No new `reqwest::Client`.
- No secret in logs, errors, problem bodies, audit details, or redirects: access and refresh tokens, codes, PKCE verifiers, `state`, client secrets, raw token responses. `connect_id` appears only in the success redirect. Token-endpoint failures log the OAuth `error` code only.
- TTLs: state and pending result 10 minutes (`STATE_TTL = Duration::minutes(10)`); refresh window 15 minutes; `refresh_credentials` schedule `0 */5 * * * *`; `oauth_state_purge` schedule `0 0 * * * *` (seconds-first cron, queue `maintenance`).
- Redirect URI: `{server.public_url}/api/v1/platforms/{id}/oauth/callback`. Allowlist: `{app.public_url}/oauth-done`, `{scheme}://oauth-done` with scheme `postit-dev` (development), `postit-qa` (qa), `postit` (production), plus `oauth.return_to_allowlist`; exact string match.
- Problem code (new): `return_to_invalid` (422). Callback result codes (query values): `access_denied`, `provider_error`, `state_invalid`, `exchange_failed`, `scope_missing`, `discovery_failed`, `no_accounts`, `platform_unavailable`.
- Audit kinds: `account_connected` (exists), `account_needs_reconnect` (exists), `account_revoked_by_platform` (add).
- The unauthenticated callback and event routes are covered by the existing per-IP `rate_limit.unauthenticated` bucket (`api/src/middleware.rs::ip_rate_limit`); no new limiter. *(Resolves the spec's "builder may reuse".)*
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=D:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data` (and from `backend/crates/oauth` if it gains macros).
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`. American English everywhere.

## Review Focus

1. **The user opens the authorize URL twice, or the browser replays the callback.** The second callback with the same `state` must redirect `state_invalid` to `{app.public_url}/oauth-done` and must not touch the first pending result. Task 7 test `callback_replay_does_not_touch_the_pending_result`.
2. **Provider redirects with `error=access_denied` and no `code`, or with both `code` and `error`.** Treat any `error` as failure (`access_denied` or `provider_error`), delete the row, never exchange. Task 7 test `callback_with_error_and_code_never_exchanges`.
3. **The access token expired while the worker was down for hours and the provider issued no refresh token.** `credentials()` must answer `Authentication` and the next cron run must set `needs_reconnect`, not loop on refresh. Task 9 test `expired_without_refresh_token_needs_reconnect`.
4. **The same remote account is connected by owner A, disconnected, then connected by owner B through OAuth while A's old grant row lingers.** B's connect succeeds; A's old grant is not revoked at the provider because B's grant has the same `remote_user_id`. Task 10 test `revoke_skipped_when_another_owner_holds_the_remote_user`.
5. **`return_to` with a trailing slash, different case in the host, or `%2F`-encoded path.** All rejected (exact match), with `return_to_invalid`. Task 7 test `return_to_near_misses_are_rejected`.

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
  - `CredentialStore::refresh(&self, stale: &OAuthTokens) -> Result<OAuthTokens, AgentError>` (default `Err(Unsupported)`), `CredentialStore::account_token(&self, account: &AccountRef) -> Result<Option<AccountToken>, AgentError>` (default `Ok(None)`).
  - All new types re-exported from `postit_core`.

- [ ] **Step 1: Write failing tests**: `spec_defaults_client_id_param` (`new()` gives `"client_id"` and empty `extra_token_params`; builders set them); `after_exchange_default_passes_tokens_through` (fake agent; result has the same `expires_at`, no `remote_user_id`, empty `account_tokens`); `debug_redacts_exchange_types` (`format!("{:?}")` of `RawTokenResponse` with `{"access_token":"tok-1"}`, of `AccountToken`, and of `GrantExchange` contains no `tok-1`); `credential_store_defaults` (`account_token` is `Ok(None)`, `refresh` is `Unsupported`); `handle_event_default_is_unsupported` updated for `EventOutcome`.
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
- In-memory credential store fake: `refresh` returns the next scripted `OAuthTokens` and counts calls (`refresh_calls()`); `account_token` returns from a map set by `with_account_token(remote_id, AccountToken)`.
- Conformance: for an agent whose `connect` is `OAuth`, `config_schema()` must exist and declare `client_id` (string) and `client_secret` (string, secret). Violation name `oauth_config_fields`.

- [ ] **Step 1: Write failing tests**: `approved_code_requires_matching_verifier` (approve, exchange with the right verifier succeeds, with a wrong one 400 `invalid_grant`), `approved_code_is_single_use`, `approved_code_requires_same_redirect_uri`, `wrong_client_is_rejected` (after `set_client`), `granted_scopes_are_reported`, `refresh_token_can_be_withheld`, `conformance_flags_oauth_agent_without_client_fields` (echo agent with an OAuth connect and no config schema fails with `oauth_config_fields`).
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit --features testkit` — FAIL.
- [ ] **Step 3: Implement.** PKCE S256 check: `BASE64URL_NOPAD(SHA256(verifier)) == challenge`.
- [ ] **Step 4: Run** the same — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): verify PKCE, redirect URI, and client in the mock OAuth provider`.

### Task 3: `mock_oauth` hooks (`postit-agent-mock`)

**Files:**
- Modify: `backend/crates/agents/mock/src/lib.rs`, `src/descriptor.rs`, `src/script.rs`, `src/client.rs` (discovery count for OAuth), new `src/oauth.rs`
- Test: `backend/crates/agents/mock/tests/oauth.rs` (create), `tests/conformance.rs` (run `mock_oauth` through conformance with its config)

**Interfaces:**
- Consumes: Task 1, Task 2.
- Produces:
  - `mock_oauth` `config_schema()`: `client_id` (string, required), `client_secret` (string, required, `x-postit-secret`).
  - `MockScript::set_exchange(ExchangeScript)` where `ExchangeScript { remote_user_id: Option<String>, reconnect_by: Option<DateTime<Utc>>, account_tokens: Vec<String> /* remote ids that get "acct-token-<id>" */, fail: bool }`. Default (unset): `remote_user_id = Some("mock-user")`, nothing else. `fail: true` returns `AgentError::Authentication`.
  - `MockScript::set_oauth_accounts(n: usize)`: OAuth discovery returns `n` accounts with remote IDs `oauth-1` … `oauth-n` (default 1, remote ID `oauth`, matching today).
  - `handle_event` for kind `"deauthorize"`: body is JSON `{"user_id": "<remote>"}`; header `X-Mock-Signature` is lowercase hex HMAC-SHA256 of the raw body keyed with `client_secret` from `ctx.config`. Valid: `EventOutcome { response: 200 empty, deauthorized: [user_id] }`. Bad or missing signature: `AgentError::Authentication`. Any other kind: `Unsupported`.
  - Module docs in `lib.rs` state that an OAuth agent should return `remote_user_id` from `after_exchange` (spec, "A grant with no remote_user_id").

- [ ] **Step 1: Write failing tests**: `exchange_defaults_to_mock_user`, `exchange_script_sets_tokens_and_reconnect_by`, `exchange_failure_is_authentication`, `oauth_discovery_returns_scripted_count`, `deauthorize_with_valid_signature_names_the_user`, `deauthorize_with_bad_signature_is_refused`, `unknown_event_kind_is_unsupported`, and `mock_oauth` passing conformance with `{"client_id":"c","client_secret":"s"}`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-mock --features testkit` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(agent-mock): script the OAuth exchange, discovery, and deauthorize events`.

### Task 4: Schema and data layer

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql` (`account_grants.credential` nullable; `oauth_states` gains `consumed_at TIMESTAMPTZ`), `backend/crates/data/src/accounts.rs`, `backend/crates/data/src/audit.rs` (`AccountRevokedByPlatform` / `account_revoked_by_platform`), `backend/crates/data/src/lib.rs`, `backend/crates/data/src/advisory_lock.rs`
- Create: `backend/crates/data/src/oauth_states.rs`
- Test: `backend/crates/data/tests/oauth_states.rs` (create), `backend/crates/data/tests/accounts.rs` (extend), existing `tests/schema.rs` and reseal tests must stay green

**Interfaces:**
- Produces in `postit_data::oauth_states`:
  - `NewOAuthState { id: OAuthStateId, state_hash: [u8; 32], user: UserId, platform: PlatformId, pkce_verifier: Option<Sealed<Value>>, return_to: String, expires_at: DateTime<Utc> }`.
  - `OAuthState { id, user, platform, pkce_verifier, return_to, expires_at, connect_id_hash: Option<Vec<u8>>, grant_tokens: Option<Sealed<Value>>, discovered: Option<Value> }`.
  - `OAuthStateRepo::seal_context(id: OAuthStateId, column: &'static str) -> SealContext`.
  - `insert(conn, NewOAuthState) -> Result<(), DataError>`.
  - `consume_state(conn, state_hash: &[u8; 32], now) -> Result<Option<OAuthState>, DataError>`: one `UPDATE … SET consumed_at = now WHERE state_hash = $1 AND consumed_at IS NULL RETURNING …`; returns the row even when expired (the service decides).
  - `store_result(conn, id, connect_id_hash: &[u8; 32], grant_tokens: &Sealed<Value>, discovered: &Value) -> Result<(), DataError>`.
  - `delete(conn, id) -> Result<(), DataError>`.
  - `find_result(conn, connect_id_hash) -> Result<Option<OAuthState>, DataError>` (no lock, no delete) and `take_result(conn, connect_id_hash) -> Result<Option<OAuthState>, DataError>` (`DELETE … RETURNING`).
  - `purge_expired(conn, now) -> Result<u64, DataError>`: deletes expired `oauth_states` and expired `pending_connections`; returns the total.
- `PendingConnectionRepo::find(conn, connect_id_hash) -> Result<Option<PendingConnection>, DataError>` (non-consuming; used by the pending read).
- `Grant.credential` becomes `Option<Sealed<Value>>`; `NewGrant.credential` stays required.
- System-side `GrantRepo` calls (no `OwnerScope`; the caller is a job or the engine, `updated_by` null):
  - `lock_for_refresh(tx, id: GrantId) -> Result<Option<(UserId, Grant)>, DataError>`: `xact_lock_pair(tx, GRANT_LOCK_NAMESPACE, key_of(id))`, then reads the grant and its owner.
  - `store_tokens(tx, id, credential: &Sealed<Value>, expires_at: Option<DateTime<Utc>>) -> Result<(), DataError>`.
  - `mark_needs_reconnect(tx, id) -> Result<Vec<(UserId, AccountId)>, DataError>` (grant and its non-disconnected accounts; returns the accounts for audit).
  - `due_for_refresh(conn, before: DateTime<Utc>, limit: i64) -> Result<Vec<GrantId>, DataError>`: active `oauth` grants with `expires_at < before`.
  - `reconnect_by_passed(conn, now) -> Result<Vec<GrantId>, DataError>` and `set_reconnect_warned(tx, id, "expired")`.
  - `revoke_by_remote_user(tx, platform, remote_user_id) -> Result<Vec<(UserId, AccountId)>, DataError>`: grants become `revoked`, `credential = NULL`, their `account_tokens` deleted, non-disconnected accounts `revoked`.
  - `other_active_grant_for_remote_user(conn, platform, remote_user_id, except: GrantId) -> Result<bool, DataError>` (any owner).
  - `active_oauth_grants_of(conn, owner: UserId) -> Result<Vec<Grant>, DataError>` and `set_revoked(tx, id)`.
- `advisory_lock::xact_lock_pair(conn, namespace: i32, key: i32)` using `pg_advisory_xact_lock(int4, int4)`; `GRANT_LOCK_NAMESPACE: i32 = 1_007`; `key_of(GrantId) -> i32` = first 4 bytes of the UUID's SHA-256, big-endian. (Two-int locks do not share the bigint key space of the existing fixed keys.)

- [ ] **Step 1: Write failing tests** (`#[sqlx::test]`): `state_consumed_once` (second `consume_state` returns `None`), `expired_state_is_returned_for_the_service_to_refuse`, `result_found_without_consuming_then_taken_once`, `purge_removes_only_expired_states_and_pending_connections`, `revoke_by_remote_user_clears_credentials_and_tokens` (two grants of the user under two owners, one of another user untouched), `mark_needs_reconnect_skips_disconnected_accounts`, `due_for_refresh_selects_active_oauth_within_window`, `other_active_grant_for_remote_user_ignores_self_and_revoked`, `grant_lock_serializes_two_transactions` (second `lock_for_refresh` blocks until the first commits, checked with a timeout), `audit_kind_account_revoked_by_platform_round_trips`. Existing `tests/schema.rs` still passes with the nullable column.
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
- Modify: `backend/crates/oauth/src/lib.rs`, `backend/crates/oauth/src/service.rs` (`AccountServiceDeps` gains `oauth_http: postit_http::Client` or the existing handle, and `public_url: Url` for the redirect URI), `backend/crates/oauth/src/testkit.rs`, `backend/crates/server/src/compose.rs` (pass them)
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

- [ ] **Step 1: Write failing tests** against `MockOAuthProvider` with `set_client`: `authorize_url_carries_pkce_state_scopes_and_extras`, `exchange_with_pkce_succeeds_and_parses_expiry`, `exchange_with_wrong_verifier_is_invalid_grant`, `post_body_and_basic_client_auth_both_work`, `custom_client_id_param_is_sent` (wiremock matcher on body `client_key=`), `refresh_without_rotation_returns_no_refresh_token`, `refresh_invalid_grant_is_classified`, `server_error_is_retryable`, `revoke_sends_token_and_hint` (provider `revoked()` has the hash), `missing_scopes_only_when_response_lists_scopes`, `token_error_display_has_no_body` (provider body contains a token-like string; `format!("{err:?}")` does not).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test engine` — FAIL.
- [ ] **Step 3: Implement** with form-encoded POSTs through the `postit-http` client. Parse JSON into `RawTokenResponse`; never log it.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): add the provider-neutral OAuth token client`.

### Task 7: Start, callback, and pending read

**Files:**
- Create: `backend/crates/oauth/src/flow.rs`, `backend/crates/oauth/src/return_to.rs`
- Modify: `backend/crates/oauth/src/lib.rs`, `src/error.rs` (`AccountError::ReturnToInvalid`), `src/service.rs` (`AccountServiceDeps.return_to: ReturnToAllowlist`), `src/connect.rs` (share `ConnectId` and `choose`)
- Test: `backend/crates/oauth/tests/oauth_flow.rs` (create), `tests/common/mod.rs` (OAuth fixtures: provider, `mock_oauth` registered with `{client_id, client_secret}`)

**Interfaces:**
- Consumes: Tasks 3, 4, 6.
- Produces (public):
  - `ReturnToAllowlist::new(app_public_url: &Url, environment: Environment, extra: &[String]) -> Self`, `fn allows(&self, candidate: &str) -> bool` (exact string equality). `Environment` is `postit_config`'s environment enum.
  - `AccountService::start_oauth(&self, scope: &OwnerScope, platform: &PlatformId, return_to: &str) -> Result<OAuthStart, AccountError>`; `OAuthStart { authorize_url: Url, expires_at: DateTime<Utc> }`. Errors: `Scope` (owner only), catalog errors, `ConnectMethodUnsupported`, `ReturnToInvalid`.
  - `AccountService::oauth_callback(&self, platform: &str, params: CallbackParams) -> Url` (never fails; returns the 303 target). `CallbackParams { code: Option<SecretString>, state: Option<String>, error: Option<String> }`.
  - `enum CallbackCode { AccessDenied, ProviderError, StateInvalid, ExchangeFailed, ScopeMissing, DiscoveryFailed, NoAccounts, PlatformUnavailable }` with `as_str()` giving the Global Constraints strings.
  - `AccountService::pending(&self, scope: &OwnerScope, connect_id: &str) -> Result<PendingView, AccountError>`; `PendingView { platform: PlatformId, connect: ConnectKind, expires_at: DateTime<Utc>, discovered: Vec<DiscoveredAccount> }`. Looks in `pending_connections` then `oauth_states`; a foreign owner's read deletes the row (`take_result` / `PendingConnectionRepo::take`) and returns `NotFound`; expired or unknown → `NotFound`.
- Callback order is the spec's steps 1–6. The stored `grant_tokens` is the sealed JSON of an internal `StoredExchange { access_token, refresh_token, expires_at, scopes, remote_user_id, reconnect_by, account_tokens: {remote_id: {access_token, expires_at}} }`, sealed with `OAuthStateRepo::seal_context(id, "grant_tokens")`. Discovery uses `after_exchange`'s tokens through an in-memory `CredentialStore` (`Credentials::OAuth`). No pooled connection is held across a provider call.

- [ ] **Step 1: Write failing tests** (`#[sqlx::test]`, provider + `mock_oauth`):
  - `start_returns_authorize_url_and_stores_hashed_state` (the URL's `state` is not stored in clear; `pkce_verifier` sealed).
  - `start_refuses_delegate_and_credentials_platform_and_unconfigured_platform`.
  - `return_to_allowlist_accepts_exact_entries` (web, `postit-dev://oauth-done` in development, an extra entry) and `return_to_near_misses_are_rejected` (`…/oauth-done/`, `…/oauth-done?x=1`, `…/oauth-done#a`, `HTTPS://POSTIT.LOCAL:44315/oauth-done`, `%2Foauth-done`, `postit://oauth-done` in development, another host) — all `ReturnToInvalid`.
  - `callback_success_redirects_with_connect_id` (target is `return_to?result=ok&connect_id=<43 chars>`; row has `connect_id_hash`, sealed `grant_tokens`, `discovered`; no grant or account rows exist).
  - `callback_unknown_expired_and_wrong_platform_state_redirect_state_invalid` to `{app.public_url}/oauth-done?result=error&code=state_invalid`.
  - `callback_replay_does_not_touch_the_pending_result`.
  - `callback_with_error_and_code_never_exchanges` (`error=access_denied` → `access_denied`; `error=temporarily_unavailable` → `provider_error`; provider saw no token request; row deleted).
  - `callback_exchange_failure`, `callback_scope_missing` (`set_granted_scopes(Some(["read"]))`), `callback_discovery_failure` (`ExchangeScript { fail: true }` → `discovery_failed`), `callback_no_accounts` (`set_oauth_accounts(0)`), `callback_platform_switched_off_mid_flow` — each redirects with its code to `return_to` and leaves no row.
  - `pending_read_by_owner_does_not_consume`, `pending_read_by_other_user_deletes_and_404s`, `pending_read_works_for_credential_connections`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test oauth_flow -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): start OAuth connections, handle the callback, and read pending results`.

### Task 8: Confirm OAuth results

**Files:**
- Modify: `backend/crates/oauth/src/connect.rs` (`confirm` looks in both tables; `ConnectInput` gains `kind: GrantKind`, `remote_user_id`, `expires_at`, `reconnect_by`, `scopes`, `account_tokens: BTreeMap<String, AccountToken>`), `backend/crates/oauth/src/manage.rs` (`ConnectKind::OAuth` if missing)
- Test: `backend/crates/oauth/tests/oauth_confirm.rs` (create)

**Interfaces:**
- Consumes: Task 7 (`StoredExchange`, `take_result`).
- Produces: `Confirmed.connect == ConnectKind::OAuth` for OAuth results; grant row `kind = 'oauth'` with `remote_user_id`, `expires_at`, `reconnect_by`, `scopes`; accounts carry the grant's scopes; `account_tokens` rows for chosen accounts that have one (sealed with `AccountTokenRepo::seal_context`). Moving an account deletes its previous unused grant and never revokes it.

- [ ] **Step 1: Write failing tests**:
  - `confirm_creates_oauth_grant_and_chosen_accounts_only` (3 discovered, 2 chosen).
  - `several_accounts_share_one_grant_token` (both accounts' `PgCredentialStore::credentials()` return the same access token).
  - `confirm_by_other_user_is_404_and_discards`.
  - `expired_result_creates_nothing`.
  - `attacker_initiated_connection_attaches_nothing`: attacker starts; the victim's browser completes the callback; the attacker confirms with a guessed or absent `connect_id` → `NotFound`; the victim confirms the real `connect_id` → `NotFound` (not the starter) and the row is gone; no grant or account exists for either user.
  - `account_connected_elsewhere_is_skipped_and_counted` (another owner holds `oauth-2`).
  - `reconnect_moves_account_and_deletes_old_grant_without_revoke` (provider `revoked()` stays empty).
  - `account_tokens_stored_for_chosen_accounts` (`ExchangeScript { account_tokens: ["oauth-1"] }`; `account_token(&oauth-1)` returns `acct-token-oauth-1`, `oauth-2` none).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test oauth_confirm -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement**; keep B3's `tests/connect.rs` green.
- [ ] **Step 4: Run** `cargo test -p postit-oauth -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): confirm OAuth connections into grants, accounts, and account tokens`.

### Task 9: Refresh and the credential store

**Files:**
- Create: `backend/crates/oauth/src/refresh.rs`, `backend/crates/oauth/src/jobs.rs`
- Modify: `backend/crates/oauth/src/store.rs` (OAuth arms), `backend/crates/oauth/Cargo.toml` (`postit-jobs`), `backend/crates/oauth/src/lib.rs`
- Test: `backend/crates/oauth/tests/refresh.rs` (create), `tests/store.rs` (extend)

**Interfaces:**
- Consumes: Task 4 `GrantRepo::lock_for_refresh`, `store_tokens`, `mark_needs_reconnect`, `due_for_refresh`, `reconnect_by_passed`; Task 6 `engine::refresh`.
- Produces:
  - `AccountService::refresh_grant(&self, grant: GrantId, stale: Option<&OAuthTokens>) -> Result<OAuthTokens, AgentError>`: the spec's four steps. Comparison "tokens differ from `stale`" is by `expires_at` and a SHA-256 of the access token, never by printing them. `stale = None` (cron) always refreshes when still due after the lock.
  - `PgCredentialStore` needs the service's engine pieces: construct it from `AccountService` (`PgCredentialStore::new(service: AccountService, owner, grant)` replacing the pool+vault constructor; update callers). `credentials()` returns `Credentials::OAuth` for `kind = oauth`, refreshing first when `expires_at <= now`; `refresh(stale)` calls `refresh_grant`; `store_refreshed(tokens)` writes under the grant lock; `account_token(account)` reads `account_tokens` by `(grant's owner, remote id)` and opens it.
  - `RefreshCredentials` job (`JOB_TYPE = "refresh_credentials"`, `Queue::Maintenance`) and `OAuthStatePurge` (`"oauth_state_purge"`, `Queue::Maintenance`); `postit_oauth::jobs::register(registry, service: AccountService, schedules: &JobSchedules) -> Result<(), JobsError>` and `postit_oauth::jobs::console(catalog, schedules)`.
  - The cron: `due_for_refresh(now + 15 min)` in batches of 100, one `refresh_grant(id, None)` each; grants with no refresh token and `expires_at <= now` go to `needs_reconnect`; then `reconnect_by_passed(now)` → `mark_needs_reconnect` and `reconnect_warned = 'expired'`. Each `needs_reconnect` records `account_needs_reconnect` per account with no actor.

- [ ] **Step 1: Write failing tests**:
  - `cron_refreshes_grants_expiring_within_15_minutes_only`.
  - `on_demand_refresh_after_401` (store `refresh(stale)` returns new tokens; `credentials()` then returns them).
  - `rotation_is_stored` and `no_rotation_keeps_the_old_refresh_token`.
  - `invalid_grant_sets_needs_reconnect_with_audit` and `expired_refresh_token_sets_needs_reconnect` (`OAuthFailure::ExpiredRefreshToken`).
  - `server_error_leaves_grant_active`.
  - `expired_without_refresh_token_needs_reconnect` (Review Focus 3).
  - `concurrent_refresh_across_two_pools_makes_one_request`: two `PgPool`s on the same database, two services, `tokio::join!` of `refresh_grant(id, Some(&stale))`; `provider.token_requests() == 1` and both return the same new token.
  - `reconnect_by_passed_sets_needs_reconnect`.
  - `account_token_is_served_for_its_account_only`.
  - `purge_job_deletes_expired_rows`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test refresh -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-oauth -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): refresh OAuth grants under a per-grant lock and add the OAuth jobs`.

### Task 10: Revocation, platform events, and the deletion step

**Files:**
- Create: `backend/crates/oauth/src/revoke.rs`, `backend/crates/oauth/src/events.rs`
- Modify: `backend/crates/oauth/src/disconnect.rs` (`revoke_and_delete` → `revoke_grant`), `backend/crates/identity/src/deletion.rs`, `backend/crates/identity/src/jobs.rs` (`IdentityJobs.deletion_steps`), `backend/crates/identity/src/lib.rs`
- Test: `backend/crates/oauth/tests/revoke.rs`, `backend/crates/oauth/tests/events.rs` (create), `backend/crates/identity/tests/deletion.rs` (extend)

**Interfaces:**
- Consumes: Task 4 `other_active_grant_for_remote_user`, `revoke_by_remote_user`, `active_oauth_grants_of`, `set_revoked`; Task 6 `engine::revoke`.
- Produces:
  - `AccountService::revoke_grant(&self, owner: UserId, grant: &Grant)` (best effort, no error): OAuth with `remote_user_id = None` → no provider call; another active grant of the same platform and remote user → no call; `spec.revoke_url` set → `engine::revoke` (refresh token, hint `refresh_token`; else access token, hint `access_token`); otherwise `client.revoke()`. Credentials grants → `client.revoke()` as in B3.
  - `AccountService::handle_platform_event(&self, platform: &str, event: InboundEvent) -> Result<EventResponse, AccountError>`: `Unsupported` → `NotFound`; agent `Authentication` → new `AccountError::EventRejected`; applies `revoke_by_remote_user` per name in one transaction with `account_revoked_by_platform` audit events (details `account_id`, `platform`); returns `outcome.response`.
  - In `postit-identity`: `#[async_trait] pub trait UserDeletionStep: Send + Sync { fn name(&self) -> &'static str; async fn run(&self, user: UserId) -> Result<(), JobError>; }`; `DeleteUserHandler::with_steps(self, Vec<Arc<dyn UserDeletionStep>>) -> Self`; steps run in order after step 1, before step 2, each logged by `name()`. `IdentityJobs.deletion_steps: Vec<Arc<dyn UserDeletionStep>>`.
  - `postit_oauth::OAuthDeletionStep::new(service: AccountService)` (`name() = "oauth"`): for each `active_oauth_grants_of(user)`: `revoke_grant`, then `set_revoked`.

- [ ] **Step 1: Write failing tests**:
  - `disconnecting_one_of_two_accounts_keeps_the_grant_unrevoked`, `disconnecting_the_last_account_revokes_at_revoke_url` (`provider.revoked()` has the refresh token hash).
  - `revoke_skipped_without_remote_user_id`, `revoke_skipped_when_another_owner_holds_the_remote_user` (Review Focus 4), `agent_revoke_used_when_spec_has_no_revoke_url` (spec without `revoke_url`; mock `revoke_calls() == 1`).
  - `deauthorize_event_revokes_matching_grants`: two owners' grants of `mock-user` and one of `other-user`; valid signed event → first two `revoked`, `credential IS NULL`, accounts `revoked`, account tokens gone, audit events written; `other-user` untouched.
  - `deauthorize_with_bad_signature_changes_nothing` (`EventRejected`), `event_kind_unsupported_is_not_found`.
  - Identity: `deletion_steps_run_in_order_and_resume` (two recording steps; `failing_after(1)` then retry runs both steps again idempotently), and in `postit-oauth`: `oauth_deletion_step_revokes_once` (run twice; provider sees one revoke).
- [ ] **Step 2: Run** `cargo test -p postit-oauth -p postit-identity -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(oauth): revoke grants safely, apply platform deauthorizations, and add the OAuth deletion step`.

### Task 11: API routes

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs`, `backend/crates/api/src/routes/mod.rs`, create `backend/crates/api/src/routes/platforms.rs`, `backend/crates/api/src/dto_accounts.rs`, `backend/crates/api/src/error.rs` (`ErrorCode::ReturnToInvalid` 422; `AccountError::ReturnToInvalid` → it; `EventRejected` → 401 with no detail), `backend/crates/api/src/openapi.rs`
- Test: `backend/crates/api/tests/oauth.rs` (create)

**Interfaces:**
- Consumes: Tasks 7, 8, 10.
- Produces:
  - `POST /api/v1/accounts/connect/{platform}` body `{ "return_to": string }` → 200 `{ "authorize_url": string, "expires_at": string }`. Scope extractor; owner only.
  - `GET /api/v1/accounts/connect/{connect_id}` → 200 `{ "platform", "connect": "oauth"|"credentials", "expires_at", "discovered": [DiscoveredAccount] }`. Owner only. Must not collide with `POST /accounts/connect/{connect_id}/confirm` or the new POST (axum matches by method; one path pattern `/accounts/connect/{id}` serves GET and POST).
  - `GET /api/v1/platforms/{id}/oauth/callback` (query `code`, `state`, `error`, `error_description` ignored) → 303 `Location`, headers `Cache-Control: no-store`, `Referrer-Policy: no-referrer`. No bearer; not behind `Scope`.
  - `POST /api/v1/platforms/{id}/events/{kind}` raw body (limit 64 KiB via its own `RequestBodyLimitLayer`, constant `EVENT_BODY_LIMIT`) → the agent's status, `Content-Type`, and body; 404 `not_found` for unsupported; 401 for rejected.
  - OpenAPI: the callback operation's description lists the result codes from Global Constraints.

- [ ] **Step 1: Write failing tests** (server testkit with `mock_oauth` and the provider):
  - `full_flow_over_http`: start → `provider.approve(authorize_url)` → `GET callback` (303 to `return_to?result=ok&connect_id=…`, `no-store`, `no-referrer`) → `GET /accounts/connect/{id}` lists accounts → confirm → `GET /accounts` shows them.
  - Isolation, four callers on start, pending read, and confirm: owner OK; delegate at `view`, `edit`, `publish` → 403 `owner_only`; delegate limited to other accounts → 403 `owner_only`; unrelated user → 404 on pending read and confirm, and start creates a flow in their own workspace only.
  - `return_to_invalid_is_422`.
  - `callback_needs_no_bearer_and_ignores_act_as`.
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

- [ ] **Step 1: Write failing tests**: `worker_registers_oauth_recurring_jobs`, `delete_user_revokes_oauth_grants` (user with a `mock_oauth` grant → delete → provider saw one revoke and the user's rows are gone).
- [ ] **Step 2: Run** `cargo test -p postit-server -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(server): register the OAuth jobs, return_to allowlist, and deletion step`.

### Task 13: Contract, clients' error maps, docs, deferred file, and gates

**Files:**
- Regenerate: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/` (`cargo xtask openapi`, from PowerShell; fix any `mobile/lib/state` breakage from renamed inline types)
- Modify: `web/src/i18n/en-US/common.json`, `web/src/i18n/ar/common.json` (`return_to_invalid`), `web/src/features/audit/kinds.ts` and `web/src/i18n/{en-US,ar}/audit.json` (`account_revoked_by_platform`), `mobile/lib/l10n/app_en.arb`, `app_ar.arb`, `mobile/lib/ui/errors/messages.dart` (`return_to_invalid`)
- Modify docs: `docs/deployment.md` (registering a platform developer app: the redirect URI; dev tunnel fallback with cloudflared or ngrok as `server.public_url`; a per-provider redirect-URI rules table for TikTok, X, YouTube/Google, LinkedIn, Meta with "verify at the F phase start" and a link to each provider's doc), `docs/security.md` (confirm-step rationale, revoke-skip rule, grants without `remote_user_id`), `CLAUDE.md` (project state: B7 built, one sentence like B3's)
- Create: `docs/superpowers/plans/2026-10-28-b7-oauth-connection-deferred.md`

- [ ] **Step 1: Run** `cargo xtask openapi` then `cargo xtask openapi --check` — PASS; web `pnpm` type check and lint per `web/CLAUDE.md`; in `mobile/`, `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test` — PASS.
- [ ] **Step 2: Write** the docs and the deferred file. Deferred entry: *Owner notifications for `needs_reconnect`, `account_revoked_by_platform`, and the `reconnect_by` warnings at 7 days and 1 day (`reconnect_warned` stages `7d`, `1d`). Why not now: they need `postit-notify`, which D1 creates. Trigger: D1 (its notification kinds and the `refresh_credentials` cron gain the warning stages).* Add any other item that truly cannot be fixed in B7, with its trigger; fix everything else.
- [ ] **Step 3: Run every gate** from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3` (background; 30+ minutes) — all PASS.
- [ ] **Step 4: Commit** `docs: describe OAuth connections and record what waits for D1` (contract regeneration may be its own `chore(api): regenerate the contract` commit).
