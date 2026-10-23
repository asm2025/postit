# B3: Account Connection with Credentials Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user paste a platform credential, see the accounts it reaches, connect the chosen ones (singly or by CSV), and list, check, rename, and disconnect them, with no platform code in `postit-oauth` and no secret in any output.

**Architecture:**
- New crate `postit-oauth` owns `PlatformCatalog` (moved from `postit-server`), `PgCredentialStore`, and `AccountService`, which drives the agent's `verify_credentials` and `account_info` and the `postit-data` repositories.
- `postit-api` adds the B3 account routes over `AccountService`; `postit-server` builds the service with "not available yet" media and link-signer stubs.
- Small supporting changes: `AccountRef.remote_id` and `CredentialFields::from_parts` in core, `HttpHandle` impls in `postit-http`, repository changes and four audit kinds in `postit-data`, `[accounts]` settings in `postit-config`.

**Tech Stack:** Rust 2024, axum, utoipa, sqlx (Postgres), `postit-vault`, `emixcrypto` (SHA-256), `emixdb` pagination, `csv-async` (new, with `tokio` feature), `rand` for the connect ID, wiremock-free test agent.

**Spec:** `docs/superpowers/specs/2026-10-23-b3-credential-connection-design.md`, which argues from `!ref/plans/03. publishing roadmap.md` phase B3. Read both. Every **(call)** in the spec is decided; do not reopen it.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests).
- Dependency order: `postit-http` gains `postit-core`; `postit-oauth` depends only on `postit-config`, `postit-core`, `postit-http`, `postit-data`, `postit-vault`, `postit-jobs` (if needed), plus external crates. It never depends on `agent-kit`, `media`, `notify`, `identity`, or `api`.
- New external dependencies go in `[workspace.dependencies]` with a `"<major>"` spec: `csv-async = { version = "1", default-features = false, features = ["tokio"] }`. No dependency with an Arabic or Arabic-derived name. Prefer a maintained library over hand-rolled parsing.
- Single migration policy: no new migration file. If an index is needed, edit `server/crates/data/migrations/0001_init.sql` in place.
- No secret (fields marked `x-postit-secret`, the sealed credential, the `connect_id`) in a log, span, error `Display`/`Debug`, problem body, CSV report, audit row, or OpenAPI example. Debug impls on types holding them redact.
- No platform IDs or platform names in `postit-oauth` outside its `testkit`.
- Rows outside a delegate's account limit behave as not found (404). `ScopeError::OwnerOnly` is 403 `owner_only`; `ScopeError::InsufficientAccess` is 403 `capability_required`.
- After every task, from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. The last task runs every gate in `CLAUDE.md` plus `cargo xtask openapi --check`.
- Commits: Conventional Commits, scope = crate touched (`core`, `http`, `data`, `config`, `oauth`, `api`, `server`, `docs`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- American English in code, docs, and messages.

## Review Focus

1. **A credential value with a secret in a non-secret-looking spot**, such as a Slack webhook URL pasted into a plain `display_name` CSV column or a secret field echoed in a schema violation. Violations name the field and code only, never the value. Task 9 tests a CSV whose `display_name` holds the secret and asserts the report never contains it.
2. **The same remote account twice in one CSV**, or in one discovery. The second row reports `already_connected` and nothing errors or duplicates. Task 9 tests it.
3. **Confirm racing confirm** with the same `connect_id` from two requests. Exactly one creates accounts; the other gets 404. Task 6 tests it with two concurrent calls.
4. **A CSV with a UTF-8 BOM, CRLF line ends, or a header with surrounding spaces.** Headers are trimmed and a leading BOM is ignored, so a spreadsheet export works. Task 9 tests it.
5. **An admin switching the platform off between connect and confirm.** Confirm re-checks availability and answers 409 `platform_disabled`, creating nothing and deleting the pending row. Task 6 tests it.

---

### Task 1: Core contract additions

**Files:**
- Modify: `server/crates/core/src/account.rs`, `server/crates/core/src/schema/credentials.rs`, every construction of `AccountRef` (`rg "AccountRef \{" server/crates`), `server/crates/core/tests/wire_format.rs` if it covers `AccountRef`
- Test: unit tests in `credentials.rs`; existing core tests

**Interfaces:**
- Produces:
  - `AccountRef { platform: PlatformId, account: AccountId, remote_id: String }` (`remote_id` = `connected_accounts.remote_account_id`).
  - `CredentialFields::from_parts(plain: serde_json::Map<String, Value>, secret: BTreeMap<String, SecretString>) -> CredentialFields`: no validation, plain entries first in map order, then secrets.
  - `CredentialFields::split(&self) -> (serde_json::Map<String, Value>, BTreeMap<String, String>)` exposing secrets for sealing; documented as "for the credential store only". Returning plain `String` for secrets is required because `Vault::seal` serializes; the caller drops the map right after sealing.

- [ ] **Step 1: Write failing tests** `from_parts_round_trips_split` (parse a schema with `server_url` plain and `token` secret, `split`, `from_parts`, assert `get_plain("server_url")` and `get_secret("token").expose_secret()` match) and `debug_of_from_parts_redacts` (`format!("{:?}")` contains `[redacted]`, not the token).
- [ ] **Step 2: Run** `cargo test -p postit-core credentials` — FAIL (no `from_parts`).
- [ ] **Step 3: Implement** both methods and add `remote_id` to `AccountRef`; fix every constructor (core's fake agent, validation engine tests).
- [ ] **Step 4: Run** `cargo test -p postit-core` — PASS.
- [ ] **Step 5: Commit** `feat(core): carry the remote account on AccountRef and rebuild stored credential fields`.

### Task 2: `HttpHandle` implementations in `postit-http`

**Files:**
- Modify: `server/crates/http/Cargo.toml` (add `postit-core.workspace = true`), `server/crates/http/src/client.rs`, `server/crates/http/src/lib.rs`, `!ref/plans/01. vision and architecture.md` (level 2 line becomes `http (config, core)`)
- Test: `server/crates/http/tests/handles.rs` (new)

**Interfaces:**
- Produces:
  - `pub struct StandardClient(pub reqwest::Client)` implementing `postit_core::HttpHandle`.
  - `impl postit_core::HttpHandle for PublicClient`.
  - `pub fn agent_handles(standard: reqwest::Client, public: PublicClient) -> postit_core::HttpHandles` (`default` = standard, `user_hosts` = public).

- [ ] **Step 1: Write failing test** `handles_downcast_to_their_clients`: build both clients with test settings, call `agent_handles`, assert `handles.default.as_any().downcast_ref::<StandardClient>().is_some()` and `handles.user_hosts.as_any().downcast_ref::<PublicClient>().is_some()`.
- [ ] **Step 2: Run** `cargo test -p postit-http --test handles` — FAIL.
- [ ] **Step 3: Implement** the newtype, the two impls, and `agent_handles`; edit plan 01's dependency line.
- [ ] **Step 4: Run** `cargo test -p postit-http` — PASS. Confirm `cargo tree -p postit-core` shows no `postit-http` (no cycle).
- [ ] **Step 5: Commit** `feat(http): expose the standard and public clients as agent HTTP handles`.

### Task 3: Account repository changes and pending connections

**Files:**
- Modify: `server/crates/data/src/accounts.rs`
- Create: `server/crates/data/src/pending_connections.rs`; register in `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/accounts.rs`, `server/crates/data/tests/pending_connections.rs` (new), `server/crates/data/tests/scope_matrix.rs` if it enumerates repositories

**Interfaces:**
- Produces:
  - `AccountUpsert::Moved { account: Account, previous_grant: GrantId }` (tuple variant becomes struct variant; `into_account` unchanged in meaning).
  - `insert_or_reactivate` on an existing row keeps `display_name` and `test_account`, updates `grant_id`, `avatar_url`, `scopes`, sets `status = 'active'`.
  - `AccountRepo::update_settings(conn, scope, id: AccountId, test_account: Option<bool>, display_name: Option<String>) -> Result<Account, DataError>`: `OwnerOnly`; `NotFound` for a missing or disconnected account.
  - `PendingConnectionRepo::seal_context(id: PendingConnectionId) -> SealContext` (`pending_connections.credential`).
  - `NewPendingConnection { id: PendingConnectionId, platform: PlatformId, connect_id_hash: [u8; 32], credential: Sealed<Value>, connection_config: Value, discovered: Value, expires_at: DateTime<Utc> }`
  - `PendingConnectionRepo::insert(conn, scope, new) -> Result<(), DataError>`: `OwnerOnly`.
  - `PendingConnection { id, owner: UserId, platform: PlatformId, credential: Sealed<Value>, connection_config: Value, discovered: Value, expires_at: DateTime<Utc> }` (Debug omits `credential`).
  - `PendingConnectionRepo::take(conn, connect_id_hash: &[u8; 32]) -> Result<Option<PendingConnection>, DataError>`: a system call; `DELETE … WHERE connect_id_hash = $1 RETURNING …` regardless of owner and expiry.

- [ ] **Step 1: Write failing tests** in `accounts.rs`: `reconnect_keeps_display_name_and_test_flag` (rename and flag an account, reconnect on a new grant, assert both kept and `Moved { previous_grant }` equals the old grant ID); `update_settings_changes_name_and_flag`; `update_settings_refuses_disconnected` (`NotFound`); isolation for `update_settings` with `callers()` (only `owner` succeeds; `view`/`edit`/`publish`/`limited` get `Scope(OwnerOnly)`; `stranger` gets `NotFound`). In `pending_connections.rs`: `take_is_single_use` (second take `None`); `take_returns_another_owners_row_and_deletes_it` (service decides; row gone after); `take_returns_expired_row` (expiry is the service's check); `insert_requires_owner`; `credential_is_sealed_for_its_row` (open with `seal_context(id)` succeeds, with another ID fails).
- [ ] **Step 2: Run** `cargo test -p postit-data --test accounts --test pending_connections` — FAIL.
- [ ] **Step 3: Implement** the changes; update every match on `AccountUpsert::Moved`.
- [ ] **Step 4: Run** `cargo test -p postit-data` — PASS (`cargo sqlx prepare` if the workspace uses offline query data: check for `server/.sqlx/`).
- [ ] **Step 5: Commit** `feat(data): keep user settings on reconnect and store pending credential connections`.

### Task 4: Audit kinds and disconnect queries

**Files:**
- Modify: `server/crates/data/src/audit.rs`, `server/crates/data/src/publishing/deliveries.rs`
- Test: `server/crates/data/tests/audit.rs`, `server/crates/data/tests/publishing_deliveries.rs`

**Interfaces:**
- Produces:
  - `AuditEventKind::{AccountConnected, AccountDisconnected, AccountNeedsReconnect, AccountsImported}` with `as_str` `account_connected`, `account_disconnected`, `account_needs_reconnect`, `accounts_imported`; `ALL` grows to 20.
  - `DeliveryRepo::account_has_in_flight(conn, account: AccountId) -> Result<bool, DataError>`: true when any delivery of the account is in a claimed or in-flight state (use the state names the B2b state machine defines; read them from `publishing/deliveries.rs`).
  - `DeliveryRepo::native_scheduled_count_for_account(conn, account: AccountId) -> Result<i64, DataError>`: same predicate as `native_scheduled_counts`, filtered to the account. (Use the actual repo type name in `deliveries.rs`/`sweep.rs`; put the query next to `native_scheduled_counts`.)

- [ ] **Step 1: Write failing tests**: audit kinds round-trip through `as_str`/parse and the DB check constraint if one exists; `in_flight_detects_claimed_delivery` and `in_flight_false_for_pending_only`; `native_scheduled_count_for_account_counts_only_that_account`.
- [ ] **Step 2: Run** the two test files — FAIL.
- [ ] **Step 3: Implement**; if `audit_events.kind` has a CHECK list in `0001_init.sql`, add the four kinds there.
- [ ] **Step 4: Run** `cargo test -p postit-data` — PASS.
- [ ] **Step 5: Commit** `feat(data): add account audit kinds and per-account delivery checks for disconnect`.

### Task 5: `postit-oauth` crate: settings, catalog, errors, test agent, credential store

**Files:**
- Modify: `server/Cargo.toml` (member, `postit-oauth` and `csv-async` workspace deps), `server/crates/config/src/settings.rs` (+ `server/config/default.toml`), `server/crates/server/src/platforms.rs`, `server/crates/server/src/lib.rs`, `docs/configuration.md`
- Create: `server/crates/oauth/Cargo.toml`, `src/lib.rs`, `src/catalog.rs`, `src/error.rs`, `src/store.rs`, `src/testkit.rs` (feature `testkit`)
- Test: `server/crates/oauth/tests/catalog.rs`, `server/crates/oauth/tests/store.rs`

**Interfaces:**
- Produces:
  - `postit_config::AccountsSettings { csv_max_bytes: u64 /* default 5_242_880 */, csv_max_rows: u32 /* default 1_000 */, csv_concurrency: u16 /* default 4, min 1 */, csv_timeout: Duration /* default 300 s */ }` as `Settings.accounts`; `RateLimitSettings.credential_checks: RateBucket` (default `rate_per_minute = 500`, `burst = 50`).
  - Moved unchanged from `postit-server`: `Availability`, `PlatformConfigs`, `resolve_platform_configs`, `availability`. `postit-server` re-imports them; its startup step and tests keep passing.
  - `pub struct PlatformCatalog { registry: AgentRegistry, configs: PlatformConfigs }` with `new`, `registry(&self)`, and `async fn resolve(&self, conn: &mut PgConnection, id: &PlatformId) -> Result<ResolvedPlatform<'_>, AccountError>` (reads the `platforms` row every call). `ResolvedPlatform { agent: &dyn Agent, config: Value }`.
  - `pub enum AccountError` (thiserror, no secret in any message): `PlatformNotFound`, `PlatformDisabled`, `PlatformUnavailable`, `ConnectMethodUnsupported`, `Validation(Vec<Violation>)`, `CredentialsInvalid { field: Option<String>, missing_scopes: Vec<String> }`, `RateLimited { retry_after: Option<Duration> }`, `NotFound`, `AccountDisconnected`, `JobActive`, `SendingMethodExists`, `CsvInvalid(String)` (names a column or line, never a value), `CsvTooLarge`, `Scope(ScopeError)`, `Data(DataError)`, `Vault(VaultError)`. `From<DataError>` maps `NotFound`/`AccountNotInScope` → `NotFound`, `Scope` → `Scope`, `SendingMethodExists` → itself. A `fn from_agent(AgentError) -> AccountError` maps `InvalidCredentials`/`Authentication`/`Authorization` → `CredentialsInvalid`, `RateLimited` → `RateLimited`, everything else → `PlatformUnavailable` (logged by variant name only).
  - `pub struct PgCredentialStore { pool: PgPool, vault: Vault, owner: UserId, grant: GrantId }` implementing `CredentialStore`: `credentials()` loads the grant, opens the sealed secret map, returns `Credentials::Fields(CredentialFields::from_parts(connection_config, secrets))`; `NotFound`, a non-`active` grant, or an `oauth` grant → `AgentError::Authentication`; `store_refreshed` → `AgentError::Unsupported` for credentials grants (B7 adds OAuth).
  - Testkit: `TestCredentialAgent` registered under any platform ID given to `TestCredentialAgent::new(platform: &str)`, schema `{ server_url: plain string, token: secret string }`, scripted with `Script` (`verify: VerifyScript::{Accounts(Vec<DiscoveredAccount>), Invalid(Option<String>), RateLimited, Fail}`, `account_info: InfoScript::{Ok, AuthFailed}`), plus `revoke_calls() -> usize` and `seen_remote_ids() -> Vec<String>` (from `account_info`). An `oauth_agent(platform)` helper returns an agent whose connect method is OAuth.

- [ ] **Step 1: Write failing tests** `catalog.rs`: `unknown_platform_not_found`, `disabled_platform_refused` (flip `platforms.enabled` between two `resolve` calls; the second fails, proving no cache), `unconfigured_platform_unavailable` (agent with a required config field, no table), `available_platform_resolves`. `store.rs`: `credentials_round_trip_plain_and_secret`, `needs_reconnect_grant_is_authentication_error`, `store_refreshed_unsupported`, `debug_of_store_and_error_never_contains_secret`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth` — FAIL.
- [ ] **Step 3: Implement**; move the catalog code with `git mv`-style history preserved where practical; update `docs/configuration.md` with `[accounts]` and `rate_limit.credential_checks`.
- [ ] **Step 4: Run** `cargo test -p postit-oauth -p postit-server -p postit-config` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): add the platform catalog, credential store, and account settings`.

### Task 6: Connect and confirm

**Files:**
- Create: `server/crates/oauth/src/service.rs`, `server/crates/oauth/src/connect.rs`
- Test: `server/crates/oauth/tests/connect.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - `pub struct AccountServiceDeps { pool: PgPool, vault: Vault, catalog: Arc<PlatformCatalog>, http: HttpHandles, media_store: Arc<dyn MediaStore>, media_host: Arc<dyn MediaHost>, link_signer: Arc<dyn LinkSigner>, clock: Arc<dyn Clock>, ids: Arc<dyn IdGenerator>, settings: AccountsSettings }`; `#[derive(Clone)] pub struct AccountService` with `AccountService::new(deps)`.
  - `pub struct ConnectId(String)`: 32 random bytes, base64url no padding; `as_str`, `hash() -> [u8; 32]` (SHA-256 via `emixcrypto`); Debug prints `ConnectId([redacted])`.
  - `start_credentials(&self, scope: &OwnerScope, platform: &PlatformId, values: serde_json::Map<String, Value>) -> Result<CredentialCheck, AccountError>`; `CredentialCheck { pending: Option<(ConnectId, DateTime<Utc>)>, discovered: Vec<DiscoveredAccount> }`. Order: `scope.require(OwnerOnly)`, catalog resolve, connect method must be `Credentials(schema)` else `ConnectMethodUnsupported`, `parse_credentials`, `verify_credentials`, then (non-empty discovery only) seal secrets for a new `PendingConnectionId` and insert with expiry now + 10 minutes.
  - `confirm(&self, scope: &OwnerScope, connect_id: &str, account_ids: Option<Vec<String>>, audit: AuditContext) -> Result<Confirmed, AccountError>`; `Confirmed { connected: Vec<(Account, AccountChange)>, skipped_elsewhere: u32, unknown: Vec<String> }`; `AccountChange::{Created, Reactivated, Moved}`. One transaction as the spec's "Connect and confirm" step 2, including availability re-check after `take`, `delete_if_unused` on each `previous_grant`, deleting the new grant when nothing connected, and one `AccountConnected` audit event per account (detail `account_id`, `platform`, `change`). Account display name and avatar come from the discovered entry.

- [ ] **Step 1: Write failing tests** (`#[sqlx::test]`, `TestCredentialAgent`): `start_returns_connect_id_and_discovered`; `start_invalid_fields_returns_every_violation`; `start_invalid_credentials_names_field`; `start_empty_discovery_returns_no_connect_id_and_stores_nothing`; `start_oauth_platform_refused`; `confirm_creates_grant_and_all_accounts`; `confirm_subset_and_unknown_ids`; `grant_reaching_several_accounts_shares_one_credential` (two accounts, one grant row, `PgCredentialStore` for it yields the token); `confirm_by_other_user_is_not_found_and_deletes_pending` (then the owner's confirm also 404); `confirm_expired_creates_nothing`; `confirm_twice_second_not_found`; `concurrent_confirms_one_wins` (Review Focus 3, `tokio::join!` on two pooled connections); `connected_elsewhere_skipped_without_owner` (assert `skipped_elsewhere == 1`, and serialize `Confirmed`'s Debug plus the audit rows: no other owner's ID); `all_skipped_leaves_no_grant`; `reconnect_moves_account_and_deletes_old_grant`; `second_email_method_is_sending_method_exists` (agent registered as `email`); `platform_disabled_between_start_and_confirm` (Review Focus 5); `secrets_never_leak` (use `postit_http::testkit::{LogCapture, assert_no_leak}` over logs, `Debug` of every returned value, and `audit_events.details`); isolation: `start` and `confirm` refused with `Scope(OwnerOnly)` for every delegate scope from `callers()`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test connect` — FAIL.
- [ ] **Step 3: Implement** `service.rs` (struct, deps, context builders `check_context(config)` and `agent_context(owner, grant, config)`) and `connect.rs`.
- [ ] **Step 4: Run** `cargo test -p postit-oauth` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): connect accounts from verified credentials with a confirm step`.

### Task 7: List, get, update, check, and mark needs-reconnect

**Files:**
- Create: `server/crates/oauth/src/manage.rs`
- Test: `server/crates/oauth/tests/manage.rs`

**Interfaces:**
- Produces:
  - `AccountView { account: Account, connect: Option<ConnectKind> }` where `ConnectKind::{Credentials, OAuth}` comes from the agent descriptor (`None` when not compiled in).
  - `list(&self, scope, platform: Option<PlatformId>, include_disconnected: bool, page: Pagination) -> Result<ResultSet<AccountView>, AccountError>` (filter disconnected in the repo query if `AccountRepo::list` cannot; add a parameter to it rather than filtering a page in memory).
  - `get(&self, scope, id: AccountId) -> Result<AccountView, AccountError>`.
  - `update(&self, scope, id, test_account: Option<bool>, display_name: Option<String>) -> Result<AccountView, AccountError>`; empty or over-100-character trimmed `display_name` → `Validation` with code `display_name_invalid`.
  - `check(&self, scope, id, audit: AuditContext) -> Result<AccountView, AccountError>`: disconnected → `AccountDisconnected`; builds `AgentContext` with `PgCredentialStore`, calls `create_client` then `account_info(&AccountRef { platform, account, remote_id })`; success → `mark_checked`; `Authentication`/`InvalidCredentials` (also from the store) → `mark_needs_reconnect`; other agent errors → `from_agent`.
  - `mark_needs_reconnect(&self, scope, grant: GrantId, audit: AuditContext) -> Result<(), AccountError>`: `GrantRepo::set_status(NeedsReconnect)` and one `AccountNeedsReconnect` event per affected account, one transaction. Doc comment states system callers pass `OwnerScope::own(grant_owner)`.

- [ ] **Step 1: Write failing tests**: `list_hides_disconnected_by_default`; `list_include_disconnected`; `list_and_get_respect_account_limit` (`limited` sees only its accounts, `stranger` sees none, `view`/`edit`/`publish` see all); `update_owner_only`; `update_rejects_blank_name`; `check_marks_checked_and_passes_remote_id` (`seen_remote_ids()` contains the account's remote ID); `check_auth_failure_moves_grant_and_all_its_accounts_to_needs_reconnect`; `check_disconnected_refused`; `check_owner_only`; `mark_needs_reconnect_writes_one_event_per_account`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test manage` — FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p postit-oauth -p postit-data` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): list, rename, and check connected accounts`.

### Task 8: Disconnect

**Files:**
- Create: `server/crates/oauth/src/disconnect.rs`
- Test: `server/crates/oauth/tests/disconnect.rs`

**Interfaces:**
- Produces: `disconnect(&self, scope, id: AccountId, audit: AuditContext) -> Result<DisconnectOutcome, AccountError>`; `DisconnectOutcome { cancelled_deliveries: u32, native_scheduled_remaining: u32 }`. Steps exactly as the spec's `DELETE /accounts/{id}` 1–4: `account_has_in_flight` → `JobActive`; transaction with `cancel_for_account` (count `Cancelled` outcomes), `native_scheduled_count_for_account`, `AccountRepo::disconnect`, `AccountDisconnected` event; after commit, if `grant_now_unused`, `create_client` + `revoke` (errors logged by variant, ignored) then `GrantRepo::delete_if_unused`.

- [ ] **Step 1: Write failing tests**: `disconnect_cancels_pending_and_parked_and_keeps_row` (seed a job with deliveries using the B2 publishing repos as `data/tests/publishing_*.rs` do); `disconnect_refused_while_in_flight` (`JobActive`, nothing changed); `disconnect_frees_remote_key_for_another_owner`; `last_account_revokes_and_deletes_grant` (`revoke_calls() == 1`, grant row gone); `one_of_two_accounts_keeps_grant` (`revoke_calls() == 0`); `revoke_failure_still_deletes_grant`; `disconnect_owner_only` for every delegate scope; `stranger_gets_not_found`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test disconnect` — FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p postit-oauth` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): disconnect accounts and revoke unused credential grants`.

### Task 9: CSV import

**Files:**
- Create: `server/crates/oauth/src/import.rs`
- Test: `server/crates/oauth/tests/import.rs`

**Interfaces:**
- Produces:
  - `import_csv<R: tokio::io::AsyncRead + Unpin + Send>(&self, scope, platform: &PlatformId, body: R, commit: bool, audit: AuditContext) -> Result<ImportReport, AccountError>`. The caller (Task 11) wraps the request body in a reader that fails with `CsvTooLarge` past `csv_max_bytes`; `import_csv` enforces `csv_max_rows` (`CsvTooLarge`) and header rules (`CsvInvalid`). Headers are trimmed, a leading BOM is stripped, CRLF accepted. Up to `csv_concurrency` rows run at once (`futures::stream::iter(..).map(..).buffered(n)`, which keeps file order); each row is parse → `verify_credentials` → (commit) the same confirm logic as Task 6 with every discovered account chosen (extract a shared `fn connect_in_tx` in `connect.rs` rather than duplicating).
  - `ImportReport { commit: bool, rows: Vec<ImportRow>, counts: BTreeMap<ImportStatus, u32> }`; `ImportRow { row: u32, status: ImportStatus, violations: Vec<String> /* codes */, field: Option<String>, account_ids: Vec<AccountId>, display_name_ignored: bool }`; `ImportStatus::{Ok, InvalidFields, InvalidCredentials, AlreadyConnected, ConnectedElsewhere, NothingFound, PlatformError}` (serde `snake_case`).
  - One `AccountsImported` audit event with `commit` and per-status counts.

- [ ] **Step 1: Write failing tests**: `report_keeps_file_order_with_concurrency` (`csv_concurrency = 4`, the test agent delays earlier rows longer); `dry_run_reports_without_creating` (no grant, account, or pending rows after); `commit_creates_valid_rows_and_reports_bad_ones` (rows: valid, invalid field, invalid credentials, nothing found; only the first creates); `duplicate_account_in_file_is_already_connected` (Review Focus 2); `connected_elsewhere_row`; `display_name_applies_to_single_account_only`; `unknown_column_rejected`, `missing_required_column_rejected`; `too_many_rows_rejected` (set `csv_max_rows = 2`); `bom_crlf_and_spaced_headers_accepted` (Review Focus 4); `oauth_platform_refused`; `report_never_contains_secret` (secret in `token` and also pasted into `display_name`, Review Focus 1, checked with `assert_no_leak` against the serialized report, logs, and audit details); `import_owner_only`.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test import` — FAIL.
- [ ] **Step 3: Implement** with `csv_async::AsyncReaderBuilder` (`trim(Trim::Headers)`, flexible off) reading `StringRecord`s.
- [ ] **Step 4: Run** `cargo test -p postit-oauth` — PASS.
- [ ] **Step 5: Commit** `feat(oauth): import credential accounts from a streamed CSV`.

### Task 10: API errors, state, and account routes

**Files:**
- Modify: `server/crates/api/Cargo.toml` (+ `postit-oauth`, dev `postit-oauth/testkit`), `src/error.rs`, `src/state.rs`, `src/routes/mod.rs`, `src/openapi.rs`, `src/testkit.rs`
- Create: `server/crates/api/src/routes/accounts.rs`, `server/crates/api/src/dto_accounts.rs`
- Test: `server/crates/api/tests/accounts.rs` (new), `server/crates/api/tests/openapi.rs`

**Interfaces:**
- Produces:
  - `ErrorCode` additions with statuses: `OwnerOnly` 403, `CapabilityRequired` 403, `CredentialsInvalid` 422, `PlatformDisabled` 409, `PlatformUnavailable` 503, `CapabilityUnsupported` 409, `SendingMethodExists` 409, `JobActive` 409. `From<DataError>` gains `Scope` → owner_only / capability_required, `AccountNotInScope` → not_found, `SendingMethodExists`, `JobActive`. `From<AccountError> for ApiError` covers every variant (`Validation` → `validation_failed` with the violations as an extension member, `CredentialsInvalid` → `field` and `missing_scopes` members, `RateLimited` → `Retry-After`, `AccountDisconnected` → `validation_failed` with detail `the account is disconnected`, `CsvTooLarge` → `payload_too_large`, `CsvInvalid` → `validation_failed`).
  - `AppState.accounts: postit_oauth::AccountService`; `Limits.credential_checks: KeyedLimiter<UserId>`.
  - Routes under `/api/v1`: `POST /accounts/credentials/{platform}` (body: JSON object), `POST /accounts/connect/{connect_id}/confirm` (`{ account_ids?: string[] }`), `GET /accounts` (`platform?`, `include_disconnected?`, pagination), `GET /accounts/{id}`, `PATCH /accounts/{id}` (`{ test_account?, display_name? }`), `DELETE /accounts/{id}`, `POST /accounts/{id}/check`. All use `Scope` and `RequestAudit`; credentials route charges `credential_checks`.
  - DTOs (`ToSchema`, no secret fields): `AccountDto { id, platform, remote_account_id, display_name, avatar_url, status, test_account, connect, last_checked_at, created_at, updated_at }`, `DiscoveredAccountDto`, `CredentialCheckResponse { connect_id?: string, expires_at?, discovered }`, `ConfirmRequest`, `ConfirmResponse { connected: [{ account, change }], skipped_elsewhere, unknown }`, `UpdateAccountRequest`, `DisconnectResponse`. OpenAPI examples use obvious placeholders (`"<bot token>"`), never realistic tokens.
  - `TestApp` builds `AccountService` with a registry the test passes in (`TestApp::start_with_agents(pool, registry)`; `start` keeps an empty registry).

- [ ] **Step 1: Write failing tests** (`api/tests/accounts.rs`, `TestCredentialAgent`): full flow credentials → confirm → list → get → patch → check → delete with status codes 200/200/200/200/200/200/200; 422 `validation_failed` body lists violations; 422 `credentials_invalid` with `field`; 404 for unknown platform; 409 `capability_unsupported` for an OAuth agent; 409 `platform_disabled`; confirm by another signed-in user 404; 409 `job_active` on delete while in flight; 429 `rate_limited` with `Retry-After` after the `credential_checks` burst (TestApp with `burst = 1`); `no_secret_in_any_response_or_log` with `assert_no_leak`; the HTTP isolation check that a second user cannot `GET /accounts/{id}` of the first (404). Add the new paths to the OpenAPI test's expectations if it lists them.
- [ ] **Step 2: Run** `cargo test -p postit-api --test accounts` — FAIL.
- [ ] **Step 3: Implement**; follow `routes/me.rs` for `utoipa::path` responses (every route lists 401, 403, 429, 500, 503 and its own codes).
- [ ] **Step 4: Run** `cargo test -p postit-api` — PASS.
- [ ] **Step 5: Commit** `feat(api): add the credential connection and account routes`.

### Task 11: Import route

**Files:**
- Modify: `server/crates/api/src/routes/accounts.rs`, `src/routes/mod.rs` (`v1_custom_limits_router`), `src/router.rs` if needed
- Test: `server/crates/api/tests/accounts.rs`

**Interfaces:**
- Produces: `POST /accounts/import/{platform}?commit=true|false` (default `false`), `Content-Type: text/csv`, registered in `v1_custom_limits_router` with body limit `accounts.csv_max_bytes` and timeout `accounts.csv_timeout` (the general 64 KiB limit and request timeout would refuse real files); the handler adapts `axum::body::Body::into_data_stream()` with `tokio_util::io::StreamReader` (add `tokio-util` if not already a workspace dependency) behind a byte-counting wrapper that returns `CsvTooLarge`. Charges `credential_checks` once. Response `ImportReportDto` mirroring `ImportReport`.

- [ ] **Step 1: Write failing tests**: dry-run 200 with per-row statuses and nothing created; commit 200 creating only valid rows; 413 `payload_too_large` past `csv_max_bytes` (set it to 64 bytes in the test); 415 or 422 for a non-CSV content type (pick 415 if the codebase has an `UnsupportedMediaType` code, else 422 `validation_failed`); report has no secret.
- [ ] **Step 2: Run** `cargo test -p postit-api --test accounts import` — FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p postit-api` — PASS.
- [ ] **Step 5: Commit** `feat(api): import credential accounts from CSV`.

### Task 12: Server wiring, generated clients, and docs

**Files:**
- Modify: `server/crates/server/Cargo.toml`, `src/compose.rs`, `src/lib.rs`, `src/serve.rs` (wherever `AppState` is built)
- Create: `server/crates/server/src/unavailable.rs` (stub `MediaStore`, `MediaHost`, `LinkSigner`)
- Regenerate: `api/openapi.json`, `web/src/api/schema.d.ts`, `app/lib/api/` via `cargo xtask openapi`
- Docs: `CLAUDE.md` (crate list gains `oauth`; project state mentions B3), `!ref/plans/01. vision and architecture.md` (level 8 `oauth` lists `config`), `!ref/plans/03. publishing roadmap.md` D1 bullets ("The disconnect flow for native-scheduled deliveries (B3 cancels pending and parked)"; "The publishing audit events, and dry-run recording (B3 adds the account events)")
- Test: `server/crates/server/tests/` startup test if present

**Interfaces:**
- Consumes: `AccountService::new`, `postit_http::agent_handles`, `PlatformCatalog::new`.
- Produces: `UnavailableMedia` implementing `MediaStore` and `MediaHost` (every call returns the `MediaError` variant meaning "not available"; pick the closest existing one), `UnavailableLinks { base: Url }` implementing `LinkSigner` (URLs under `{public_url}/unavailable/…`). Doc comments say B4 and C2 replace them.

- [ ] **Step 1: Write failing test**: the server's existing startup/compose test (or a new `compose_builds_account_service`) asserts the state has an `AccountService` and `GET /api/v1/accounts` answers 200 with an empty page.
- [ ] **Step 2: Run** `cargo test -p postit-server` — FAIL.
- [ ] **Step 3: Implement** the wiring and stubs; run `cargo xtask openapi` from `server/` (Windows: PowerShell for pnpm), then edit the docs.
- [ ] **Step 4: Run** every gate from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo xtask openapi --check`; from `web/`: `pnpm typecheck` (or the check `web/CLAUDE.md` names); from `app/`: `flutter analyze`. All PASS.
- [ ] **Step 5: Commit** `feat(server): wire the account service and regenerate the API clients` (and a separate `docs:` commit for the plan and CLAUDE.md edits).
