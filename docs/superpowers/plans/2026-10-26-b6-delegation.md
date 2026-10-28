# B6: Delegation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an owner grant another active user `view`, `edit`, or `publish` access to their workspace, optionally limited to chosen accounts and with an optional expiry, and let that delegate act through `X-Postit-Act-As` with exactly that level and limit, proven at the service and HTTP layers.

**Architecture:**
- `postit-data` gains the `delegations` and `delegation_accounts` tables, `DelegationRepo` (owner-side writes and lists, the delegate-side lock, act-as resolution, the expiry batch), a verified-email user lookup, and removal of a disconnected account from every account limit.
- `postit-identity` gains `DelegationService` (grant, change, revoke, leave, list, workspaces, lookup), `resolve_act_as`, and the `delegation_expiry` recurring job.
- `postit-api`: `Scope` is the only extractor that accepts `X-Postit-Act-As`; `Auth`, `ActiveUser`, and `RequireAdmin` refuse it with `owner_only`; the global `reject_act_as` layer shrinks to the routes that never authenticate. New routes `/me/workspaces`, `/delegations*`, `/users/lookup`. `postit-server` wires the service and the job.

**Tech Stack:** Rust 2024, axum 0.8, sqlx (Postgres), governor (via the existing `KeyedLimiter`), utoipa, chrono. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-26-b6-delegation-design.md` (approved). It argues from `!ref/plans/03. publishing roadmap.md` (Phase B6, REST surface, "Acting through a delegation", jobs and audit tables) and `!ref/plans/01. vision and architecture.md` (Delegation, Identity data model, user deletion). Read all three before starting. The spec's "My call" items are decided and not reopened.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests).
- Single migration policy: edit `backend/crates/data/migrations/0001_init.sql` in place; no new migration file.
- Owned-table conventions: `owner_id`, `created_at`, `updated_at`, `created_by`, `updated_by`; unique `(owner_id, id)`; composite FKs between owned rows; actor columns (`created_by`, `updated_by`, `revoked_by`) nullable `ON DELETE SET NULL`; `owner_id` and `delegate_id` `ON DELETE CASCADE`.
- Resolution is never cached and reads `users.status` from the table, not the principal cache.
- Error codes, exact strings: `delegation_invalid` (403, new), `delegation_exists` (409, new), `owner_only` (403), `capability_required` (403), `validation_failed` (422), `not_found` (404), `rate_limited` (429). `account_not_in_scope` (403, new): `DataError::AccountNotInScope`, the whole-object refusal of a visible object that touches accounts outside the limit, maps to it (plans 01 and 03). `DataError::NotFound` stays 404 `not_found`, so a row outside the account limit stays hidden. Service-layer tests assert `DataError::AccountNotInScope`; HTTP tests assert 404 for a hidden row and 403 `account_not_in_scope` for a whole-object refusal.
- Audit kinds, exact strings: `delegation_granted`, `delegation_changed`, `delegation_revoked`, `delegation_left`, `delegation_expired`. Any user ID in `details` goes under a key ending `_user_id`.
- Level strings in the database and the API: `view`, `edit`, `publish`. Ended reasons: `revoked`, `left`, `expired`.
- New config: `rate_limit.lookup` (default `rate_per_minute = 10`, `burst = 5`) and `jobs.schedules.delegation_expiry` (default `0 */5 * * * *`, seconds-first cron, queue `maintenance`).
- No web or mobile screens; only the regenerated contract (`backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/`).
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests need `DATABASE_URL` (see the `test-env-gotchas` memory: host `127.0.0.1`, `--test-threads=3`, `CARGO_TARGET_DIR=D:/cargo-target/postit`). New `sqlx::query!` calls need `cargo sqlx prepare` from `backend/crates/data`. The last task runs every gate in `CLAUDE.md` plus `cargo xtask openapi --check`.
- Commits: Conventional Commits, scope = crate or area (`config`, `data`, `identity`, `api`, `server`, `docs`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (or the model that did the work, per the commit-trailer memory).
- American English in code, docs, and messages.

## Review Focus

1. **A delegate's client sends the header on every request.** `GET /me`, `/me/preferences`, and `/me/workspaces` with `X-Postit-Act-As` must answer 403 `owner_only`, never act on the owner and never silently ignore it. Task 6 tests each self route with a valid grant's header.
2. **The grant ends between two requests.** Revoked, left, expired (not yet ended by the job), owner disabled, delegate disabled, owner `deleting`: the very next request with the header is 403 `delegation_invalid`. Task 4 tests resolution, Task 8 the HTTP path.
3. **Odd header values:** uppercase UUID (accepted, same as lowercase), surrounding whitespace (refused), two header values, an empty value, a non-UUID, the braced, `urn:uuid:`, and unhyphenated UUID forms (refused: only the 36-character hyphenated form is accepted), the caller's own ID, a pending user's ID. All but the uppercase form are 403 `delegation_invalid` with an identical body. Task 6 tests them.
4. **The owner disconnects the last account of a limited grant.** The grant stays live, `account_ids` is `[]`, the delegate still lists posts and media, sees no accounts, and a reconnect does not restore the account to the limit. Task 3 (data) and Task 8 (HTTP) test it.
5. **Lookup input:** mixed case, surrounding whitespace (trimmed), an unverified email, a pending user's email, the caller's own email, two active verified users sharing it: only the first two match (when the user is active and verified); every other case returns `{ "user": null }` with status 200. Task 3 (repo) and Task 7 (HTTP) test them.

---

### Task 1: Settings

**Files:**
- Modify: `backend/crates/config/src/settings.rs` (`RateLimitSettings.lookup`, `JobSchedules.delegation_expiry`), `backend/config/default.toml`, `backend/crates/config/src/loader.rs` (test TOML), `docs/configuration.md` (`rate_limit` and `jobs` sections)
- Test: existing config tests in `backend/crates/config/`

**Interfaces:**
- Produces: `RateLimitSettings.lookup: RateBucket`; `JobSchedules.delegation_expiry: String`.

- [ ] **Step 1: Write failing test** `delegation_defaults_load`: default config yields `rate_limit.lookup.rate_per_minute == 10`, `burst == 5`, `jobs.schedules.delegation_expiry == "0 */5 * * * *"`. Extend the existing zero-bucket validation test so `rate_limit.lookup.rate_per_minute = 0` is refused naming the key.
- [ ] **Step 2: Run** `cargo test -p postit-config` — FAIL.
- [ ] **Step 3: Implement** the fields, defaults, validation, and the `docs/configuration.md` rows with their `POSTIT__…` names.
- [ ] **Step 4: Run** `cargo test -p postit-config` — PASS.
- [ ] **Step 5: Commit** `feat(config): add the lookup rate limit and the delegation_expiry schedule`.

### Task 2: Delegation schema and owner-side repository

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/lib.rs`, `backend/crates/data/src/audit.rs` (five kinds), `backend/crates/data/src/scope.rs` (`Level::as_str`, `FromStr`)
- Create: `backend/crates/data/src/delegations.rs`
- Test: `backend/crates/data/tests/delegations.rs`, `backend/crates/data/tests/schema.rs` (schema from empty still passes)

**Interfaces:**
- Produces in `postit_data::delegations`:
  - `pub enum EndedReason { Revoked, Left, Expired }` with `as_str`.
  - `pub struct Delegation { pub id: DelegationId, pub owner_id: UserId, pub delegate_id: UserId, pub level: Level, pub accounts: AccountLimit, pub expires_at: Option<DateTime<Utc>>, pub ended_at: Option<DateTime<Utc>>, pub ended_reason: Option<EndedReason>, pub revoked_by: Option<UserId>, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>, pub created_by: Option<UserId>, pub updated_by: Option<UserId> }` with `fn is_live(&self, now) -> bool` (`ended_at` none and `expires_at` none or after `now`).
  - `pub struct DelegationView { pub delegation: Delegation, pub other_display_name: String, pub other_status: UserStatus }` (the delegate on the owner's list, the owner on the received list).
  - `pub struct NewDelegation { pub id: DelegationId, pub delegate_id: UserId, pub level: Level, pub accounts: AccountLimit, pub expires_at: Option<DateTime<Utc>> }`.
  - `pub struct DelegationChange { pub level: Option<Level>, pub accounts: Option<AccountLimit>, pub expires_at: Option<Option<DateTime<Utc>>> }`.
  - `DelegationRepo` (all owner-side methods start with `scope.require(Capability::OwnerOnly)?`):
    - `insert(conn, scope, new: NewDelegation) -> Result<Delegation, DataError>`: inserts the grant and, for `AccountLimit::Only`, its `delegation_accounts` rows. Every listed account must be the owner's and not `disconnected`, else `DataError::Conflict("account_unavailable")`. The check reads the account rows with `SELECT … FOR SHARE` inside the grant's transaction, so it serializes with `AccountRepo::disconnect` (which locks the row `FOR UPDATE`): without the lock, a concurrent disconnect could delete the limit rows before the grant commits a row for the now-disconnected account, and `insert_or_reactivate` reuses the account ID on reconnect, bringing the account back into the limit. The FK alone does not prevent this (it takes `FOR KEY SHARE`, which a status update does not conflict with). The partial unique index violation maps to `DataError::Conflict("delegation_exists")`.
    - `lock_live_for_pair(conn, scope, delegate: UserId) -> Result<Option<Delegation>, DataError>`: the row with `ended_at IS NULL` `FOR UPDATE`.
    - `lock(conn, scope, id) -> Result<Delegation, DataError>`: owner's grant `FOR UPDATE`, ended or not; `NotFound` for another owner's.
    - `lock_received(conn, delegate: UserId, id) -> Result<Delegation, DataError>`: system-style read for the leave path, `WHERE delegate_id = $1 AND id = $2 FOR UPDATE`.
    - `update(conn, scope, id, change: DelegationChange) -> Result<Delegation, DataError>`: applies present fields, replaces `delegation_accounts` when `accounts` is present (same locked account check as `insert`), sets `updated_by = scope.actor`.
    - `end(conn, id, reason: EndedReason, revoked_by: Option<UserId>, now: DateTime<Utc>) -> Result<(), DataError>`: sets `ended_at = $now`, `ended_reason`, `revoked_by`; no-op on an ended row. Every timestamp that decides liveness comes from the caller's `Clock`, never the database's `now()`, so tests with a fixed clock see consistent state.
    - `list_granted(conn, scope, include_ended: bool, pagination: Pagination) -> Result<ResultSet<DelegationView>, DataError>`: newest first.
    - `list_received(conn, delegate: UserId, pagination: Pagination) -> Result<ResultSet<DelegationView>, DataError>`: live only (`ended_at IS NULL`), newest first.
  - `AuditEventKind::{DelegationGranted, DelegationChanged, DelegationRevoked, DelegationLeft, DelegationExpired}` with the exact strings.

- [ ] **Step 1: Write failing tests** in `tests/delegations.rs`: `insert_all_accounts_and_read_back`; `insert_limited_stores_accounts_sorted`; `insert_empty_limit_is_allowed` (`AccountLimit::Only([])` round-trips); `insert_refuses_self` (check constraint → `DataError::Sql` with the check name, or map it to `Conflict("self_delegation")`; pick the mapping and assert it); `insert_refuses_another_owners_account` (`Conflict("account_unavailable")`); `composite_fk_rejects_cross_owner_account` (a raw `INSERT INTO delegation_accounts` naming the owner's grant and another owner's account fails with the FK violation, proving the database rule independently of the repository's pre-check); `insert_refuses_disconnected_account`; `insert_waits_for_concurrent_disconnect` (connection 1 begins `disconnect` and holds its transaction open; connection 2's `insert` limited to that account blocks, then fails with `Conflict("account_unavailable")` once connection 1 commits; no `delegation_accounts` row remains); `second_live_grant_for_pair_is_delegation_exists`; `ended_grant_frees_the_pair` (end, then insert succeeds); `update_replaces_account_limit_and_sets_updated_by`; `update_clears_expiry` (`Some(None)`); `delegated_scope_cannot_insert_update_lock_or_list` (each owner-side method with `callers().publish` → `DataError::Scope(ScopeError::OwnerOnly)`); `lock_is_not_found_for_another_owner`; `list_granted_hides_ended_unless_asked`; `list_received_shows_only_live_grants_with_owner_name_and_status`; `deleting_owner_or_delegate_cascades_grants` (`UsersRepo::delete` on each side removes the row and its accounts).
- [ ] **Step 2: Run** `cargo test -p postit-data --test delegations -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement** the tables per the spec's Schema section (`delegation_accounts(owner_id, delegation_id, account_id)` with PK `(delegation_id, account_id)` and both composite FKs cascading; `level` and `ended_reason` as `TEXT` with `CHECK`s; `CHECK ((ended_at IS NULL) = (ended_reason IS NULL))`; partial unique index `(owner_id, delegate_id) WHERE ended_at IS NULL`; index on `(delegate_id) WHERE ended_at IS NULL` for resolution and the received list; index on `(expires_at) WHERE ended_at IS NULL AND expires_at IS NOT NULL` for the job), the repository, the audit kinds; run `cargo sqlx prepare`.
- [ ] **Step 4: Run** `cargo test -p postit-data -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(data): add delegations and their owner-side repository`.

### Task 3: Resolution, expiry, lookup, and disconnect in the data layer

**Files:**
- Modify: `backend/crates/data/src/delegations.rs`, `backend/crates/data/src/users.rs`, `backend/crates/data/src/accounts.rs` (`disconnect`), `backend/crates/data/src/testkit.rs`
- Test: `backend/crates/data/tests/delegations.rs`, `backend/crates/data/tests/accounts.rs`, `backend/crates/data/tests/users.rs`, `backend/crates/data/tests/scope_matrix.rs` (and any isolation test whose `callers` call needs its accounts fixed)

**Interfaces:**
- Consumes: Task 2's types.
- Produces:
  - `DelegationRepo::resolve(conn, actor: UserId, owner: UserId, now: DateTime<Utc>) -> Result<Option<OwnerScope>, DataError>`: one query joining `delegations` to `users` twice (owner and delegate both `status = 'active'`), `ended_at IS NULL`, `expires_at IS NULL OR expires_at > now`, aggregating `delegation_accounts` (`array_agg … FILTER`); returns `OwnerScope::delegated(actor, owner, id, level, limit)`. `actor == owner` returns `None`.
  - `DelegationRepo::take_expired(conn, now, limit: i64) -> Result<Vec<Delegation>, DataError>`: one `UPDATE … SET ended_at = $now, ended_reason = 'expired' WHERE id IN (SELECT … WHERE ended_at IS NULL AND expires_at <= $now ORDER BY expires_at LIMIT $limit FOR UPDATE SKIP LOCKED) RETURNING …`.
  - `UsersRepo::find_active_verified_by_email(conn, email: &str) -> Result<Vec<(UserId, String)>, DataError>`: exact citext match, `status = 'active' AND email_verified`, `LIMIT 2`; returns `(id, display_name)`.
  - `AccountRepo::disconnect` also runs `DELETE FROM delegation_accounts WHERE owner_id = $1 AND account_id = $2` in its transaction.
  - `postit_data::testkit::grant(pool, owner: UserId, delegate: UserId, level: Level, accounts: AccountLimit) -> DelegationId` (inserts through `DelegationRepo::insert` with the owner's scope; panics on failure, test helper).
  - `postit_data::testkit::callers` changes: each delegate caller now gets a real grant through `grant`, and its scope comes from `DelegationRepo::resolve` (panicking if it resolves to `None`), not from `OwnerScope::delegated` with a made-up `DelegationId`. Every existing `scope_matrix.rs` and per-repository isolation test therefore re-runs on real resolution (the spec's "four-caller isolation matrix re-run with scopes produced by real resolution"). Callers that pass `other_accounts` must pass the owner's live accounts; fix any test that passes another owner's or a disconnected account.

- [ ] **Step 1: Write failing tests**: `resolve_valid_grant_returns_level_and_limit` (all-accounts and limited, limit sorted); `resolve_none_for_revoked_left_and_expired` (expired = `expires_at` one second before `now`, `ended_at` still null); `resolve_none_when_owner_or_delegate_disabled_pending_or_deleting`; `resolve_none_for_self_and_unknown_owner`; `resolve_reflects_level_change_immediately`; `resolve_limited_grant_with_no_accounts_is_empty_only`; `take_expired_ends_due_grants_once` (two concurrent calls on separate connections end each due grant exactly once, a future grant untouched); `find_active_verified_by_email_matches_case_insensitively`; `find_active_verified_by_email_ignores_unverified_pending_and_disabled`; `find_active_verified_by_email_returns_two_for_duplicates`; in `accounts.rs`: `disconnect_removes_account_from_every_limit` (two grants limited to it; both left with `Only([])`, still live) and `reconnect_does_not_restore_limit` (reconnect through `insert_or_reactivate`, which reuses the account ID); in `scope_matrix.rs`: `callers_resolve_from_real_grants` (each delegate scope's `DelegationId` names a live row and its level and limit match).
- [ ] **Step 2: Run** the three test files — FAIL.
- [ ] **Step 3: Implement**; run `cargo sqlx prepare`.
- [ ] **Step 4: Run** `cargo test -p postit-data -- --test-threads=3` — PASS (the whole suite, since `callers` changed).
- [ ] **Step 5: Commit** `feat(data): resolve and expire delegations, look up users by verified email, and drop disconnected accounts from limits`.

### Task 4: `DelegationService` and act-as resolution

**Files:**
- Create: `backend/crates/identity/src/delegation.rs`
- Modify: `backend/crates/identity/src/lib.rs`
- Test: `backend/crates/identity/tests/delegation.rs`, `backend/crates/identity/tests/deletion.rs`

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces in `postit_identity::delegation`:
  - `pub enum DelegationError { NotFound, Exists, Invalid { field: &'static str }, Scope(ScopeError), Data(DataError), Audit(AuditError) }` (`thiserror`; `Invalid` fields: `delegate_id`, `expires_at`, `account_ids`, `email`).
  - `pub enum AccountsInput { All, Only(Vec<AccountId>) }`.
  - `pub struct GrantRequest { pub delegate_id: UserId, pub level: Level, pub accounts: AccountsInput, pub expires_at: Option<DateTime<Utc>> }`.
  - `pub struct ChangeRequest { pub level: Option<Level>, pub accounts: Option<AccountsInput>, pub expires_at: Option<Option<DateTime<Utc>>> }` (the API maps `all_accounts: true` to `Some(All)` and `account_ids` to `Some(Only(..))`, refusing both together before calling).
  - `pub struct Workspace { pub owner_id: UserId, pub owner_display_name: String, pub access: Option<(DelegationId, Level, AccountLimit)> }` (`None` for the caller's own).
  - `pub struct DelegationService` with `new(pool: PgPool, ids: Arc<dyn IdGenerator>, clock: Arc<dyn Clock>) -> Self` and:
    - `grant(&self, scope: &OwnerScope, req: GrantRequest, ctx: AuditContext) -> Result<Delegation, DelegationError>`
    - `change(&self, scope: &OwnerScope, id: DelegationId, req: ChangeRequest, ctx) -> Result<Delegation, DelegationError>`
    - `revoke(&self, scope: &OwnerScope, id: DelegationId, ctx) -> Result<(), DelegationError>`
    - `leave(&self, delegate: UserId, id: DelegationId, ctx) -> Result<(), DelegationError>`
    - `list_granted(&self, scope: &OwnerScope, include_ended: bool, pagination: Pagination) -> Result<ResultSet<DelegationView>, DelegationError>`
    - `list_received(&self, delegate: UserId, pagination: Pagination) -> Result<ResultSet<DelegationView>, DelegationError>`
    - `workspaces(&self, user: UserId) -> Result<Vec<Workspace>, DelegationError>` (own first, then live unexpired received grants whose owner is active)
    - `lookup(&self, caller: UserId, email: &str) -> Result<Option<(UserId, String)>, DelegationError>` (trim; empty after trim → `Invalid { field: "email" }`; exactly one match that is not the caller → `Some`)
  - `pub async fn resolve_act_as(pool: &PgPool, actor: UserId, owner: UserId, now: DateTime<Utc>) -> Result<Option<OwnerScope>, DataError>` (thin wrapper over `DelegationRepo::resolve`, the one entry point the API calls).
- Rules (from the spec's Service rules): `grant` and `change` and `revoke` and the owner list call `scope.require(Capability::OwnerOnly)` first. `grant` checks the delegate exists and is `active` and is not the owner (`Invalid { delegate_id }`), `expires_at > now` (`Invalid { expires_at }`), then in one transaction: `lock_live_for_pair`; a live-but-expired row is ended with `Expired` plus a `delegation_expired` event (actor null); a live unexpired row is `Exists`; insert; `delegation_granted`. `change` on an ended or expired grant is `NotFound`; one `delegation_changed` event with `details.before` and `details.after` (`level`, `all_accounts`, `account_ids`, `expires_at`). `revoke`/`leave`: already ended → `Ok(())` with no event; expired but not ended → end with `Expired` and `delegation_expired`; otherwise end with `Revoked` (`revoked_by = actor`) or `Left` and the matching event. Audit fields: `owner_id` = grant owner, `subject` = delegate, actor = acting user; `details` holds `delegation_id`, `level`, `all_accounts`, `account_ids`, `expires_at`. `DataError::Conflict("account_unavailable")` maps to `Invalid { account_ids }`, `Conflict("delegation_exists")` to `Exists`.

- [ ] **Step 1: Write failing tests**: `grant_records_audit_event`; `grant_refuses_self_inactive_and_unknown_delegate`; `grant_refuses_past_expiry`; `grant_twice_is_exists`; `regrant_over_expired_unended_grant_ends_it_first` (two events: `delegation_expired`, `delegation_granted`); `delegated_scope_cannot_grant_change_revoke_or_list` (`Scope(OwnerOnly)` for every level: no re-delegation); `change_level_and_limit_audits_before_and_after`; `change_on_ended_or_expired_is_not_found`; `revoke_is_idempotent_with_one_event`; `leave_by_delegate_and_not_by_stranger` (stranger gets `NotFound`); `revoke_expired_unended_records_expired`; `workspaces_lists_own_then_received_hiding_inactive_owners`; `lookup_exact_self_and_duplicate` (match, caller's own email → `None`, two users → `None`, blank → `Invalid`); `resolve_after_revoke_level_change_and_disable` (resolution reflects each on the next call). In `identity/tests/deletion.rs`: `delete_user_removes_grants_in_both_directions` (a user who is owner of one grant and delegate of another goes through the `delete_user` job; both grants and their `delegation_accounts` rows are gone, and the other parties' rows are untouched).
- [ ] **Step 2: Run** `cargo test -p postit-identity --test delegation --test deletion -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-identity -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(identity): add the delegation service and act-as resolution`.

### Task 5: `delegation_expiry` job

**Files:**
- Modify: `backend/crates/identity/src/delegation.rs` (job type and handler fn), `backend/crates/identity/src/jobs.rs` (`console`, `register`), `backend/crates/server/src/` (wherever `IdentityJobs` is built, if a field is added)
- Test: `backend/crates/identity/tests/recurring_jobs.rs`, `backend/crates/identity/tests/delegation.rs`

**Interfaces:**
- Produces: `pub struct DelegationExpiry {}` implementing `Job` with `JOB_TYPE = "delegation_expiry"`; `pub async fn expire_delegations(pool: &PgPool, ids: &dyn IdGenerator, now: DateTime<Utc>) -> Result<u64, IdentityError>` (loops `take_expired` in batches of 200, one `delegation_expired` audit event per grant with actor null, each batch in one transaction; returns the count). Registered on `schedules.delegation_expiry` with `maintenance_retry()`, added to `console` with `.recurring(&schedules.delegation_expiry)`.

- [ ] **Step 1: Write failing tests**: `expire_delegations_ends_due_grants_and_audits` (three due, one future; returns 3; three `delegation_expired` events with null actor); extend the recurring-jobs registration test to expect `delegation_expiry` with its schedule.
- [ ] **Step 2: Run** `cargo test -p postit-identity --test delegation --test recurring_jobs -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-identity -- --test-threads=3` and `cargo check -p postit-server` — PASS.
- [ ] **Step 5: Commit** `feat(identity): end expired delegations on a schedule`.

### Task 6: Act-as in the API extractors

**Files:**
- Modify: `backend/crates/api/src/extract.rs`, `backend/crates/api/src/middleware.rs`, `backend/crates/api/src/router.rs`, `backend/crates/api/src/routes/mod.rs`, `backend/crates/api/src/error.rs` (codes `DelegationInvalid`, `DelegationExists`; `From<DelegationError>`), `backend/crates/api/src/openapi.rs` if responses are listed centrally
- Test: `backend/crates/api/tests/hardening.rs`, `backend/crates/api/tests/preferences.rs`, `backend/crates/api/tests/act_as.rs` (new)

**Interfaces:**
- Consumes: Task 4's `resolve_act_as`.
- Produces:
  - `ErrorCode::DelegationInvalid` (403, `delegation_invalid`, title "Delegation is not valid") and `ErrorCode::DelegationExists` (409, `delegation_exists`, title "A delegation to this user already exists").
  - `From<DelegationError> for ApiError`: `NotFound` → 404, `Exists` → 409 `delegation_exists`, `Invalid { field }` → 422 with `field`, `Scope` → existing scope mapping (`owner_only` / `capability_required`), others internal.
  - Extractors: a private `async fn active_principal(parts, state) -> Result<Principal, ApiError>` (today's `ActiveUser` body). `Auth`, `ActiveUser`, `RequireAdmin` refuse a request carrying `x-postit-act-as` with 403 `owner_only` after authentication succeeds (so a bad token stays 401). `Scope`: no header → `OwnerScope::own`; header → exactly one value that is exactly 36 bytes, parses with `Uuid::try_parse` on the raw value (no trimming), and so is the hyphenated form only (`Uuid::try_parse` alone also accepts the braced, `urn:uuid:`, and unhyphenated forms, which the length check refuses), and `resolve_act_as(&state.pool, principal.user_id, owner, state.clock.now())` returns `Some`, else 403 `delegation_invalid` with no detail.
  - `middleware::reject_act_as` answers 403 `owner_only` and is applied only to the routes with no auth extractor: `/health`, `/ready`, the docs router, the public media router, `/api/v1/auth/config`, and the unauthenticated `/_test` routes (`sleep`, `echo`, `marked`, built with the testing feature) (route layers). The global layer in `api_router` is removed. The `Scope` doc comment drops the "Until plan 03 B6" sentence.

- [ ] **Step 1: Write failing tests** in `act_as.rs` (seed owner, delegates via `postit_data::testkit::grant`, tokens via `TestApp::token`): `own_scope_without_header` (`GET /api/v1/accounts` 200); `valid_header_acts_in_owner_workspace` (a view delegate lists the owner's media); `uppercase_uuid_header_is_accepted`; `invalid_headers_are_delegation_invalid_with_identical_bodies` (for: non-UUID, empty, `" <uuid>"`, `"{<uuid>}"`, `"urn:uuid:<uuid>"`, the 32-character unhyphenated form, two values, own ID, unknown ID, a pending user's ID, a revoked grant's owner — every body's `code == "delegation_invalid"` and the bodies are equal after removing `instance`/request ID); `self_routes_refuse_header_as_owner_only` (`GET /me`, `GET/PUT /me/preferences` with a valid grant's header → 403 `owner_only`); `admin_routes_refuse_header_as_owner_only` (admin with a valid grant from another user calls `GET /admin/jobs` → 403 `owner_only`); `bad_token_with_header_is_unauthenticated` (401); `unauthenticated_routes_refuse_header` (`/health`, `/api/v1/auth/config`, `/public/media/{id}` → 403 `owner_only`); `every_unsecured_route_refuses_header` (walks the generated OpenAPI document, and for every operation without a `security` requirement sends the header with path parameters filled by placeholders and asserts 403 `owner_only`, so a future unauthenticated route that misses the layer fails this test). Rewrite `hardening.rs`'s `act_as_is_forbidden_everywhere_until_delegation_exists` into `act_as_without_grant_is_refused` (`delegation_invalid` on scoped routes, `owner_only` on the others) and keep `act_as_is_charged_to_the_ip_bucket` using `/health`. Update `preferences.rs`'s `act_as_header_does_not_reach_another_user` to expect `owner_only`.
- [ ] **Step 2: Run** `cargo test -p postit-api --test act_as --test hardening --test preferences -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p postit-api -- --test-threads=3` — PASS.
- [ ] **Step 5: Commit** `feat(api): resolve X-Postit-Act-As in Scope and refuse it everywhere else`.

### Task 7: Delegation, workspace, and lookup routes

**Files:**
- Create: `backend/crates/api/src/routes/delegations.rs`, `backend/crates/api/src/dto_delegations.rs`
- Modify: `backend/crates/api/src/routes/mod.rs`, `backend/crates/api/src/routes/me.rs` (`/me/workspaces`), `backend/crates/api/src/routes/users.rs` (`/users/lookup`), `backend/crates/api/src/state.rs` (`AppState.delegations: DelegationService`, `Limits.lookup: KeyedLimiter<UserId>` built in `Limits::new` and swept in `Limits::retain_recent`, so its key map cannot grow without bound), `backend/crates/api/src/testkit.rs`, `backend/crates/api/src/openapi.rs`, `backend/crates/server/src/` (build `DelegationService` into `AppState`)
- Test: `backend/crates/api/tests/delegations.rs` (new)

**Interfaces:**
- Consumes: Task 4's service, Task 6's error mapping.
- Produces (DTO field names are the builder's; these are the shapes):
  - `DelegationDto { id, owner: UserRef, delegate: UserRef, level, all_accounts, account_ids: Vec<Uuid>, expires_at, ended_at, ended_reason, created_at, created_by, updated_at, updated_by }` with `UserRef { id, display_name, status }`; `level` and `ended_reason` as lowercase string enums in the schema.
  - `GET /api/v1/delegations?include_ended=&page…` → `Page<DelegationDto>` (takes `Scope`; owner-only via the service).
  - `POST /api/v1/delegations` body `{ delegate_id, level, account_ids?, expires_at? }` → 201 `DelegationDto`.
  - `PATCH /api/v1/delegations/{id}` body `{ level?, all_accounts?, account_ids?, expires_at? }` (`expires_at: null` clears; `all_accounts` must be `true` when present, `false` → 422 `field: "all_accounts"`; both `all_accounts` and `account_ids` → 422 `field: "account_ids"`) → 200 `DelegationDto`. `expires_at` is tri-state: absent leaves it unchanged, `null` clears it, a timestamp sets it. The DTO field is `Option<Option<DateTime<Utc>>>` with `#[serde(default, deserialize_with = …)]` (a small local helper that wraps the inner `Option` in `Some`, so a present `null` becomes `Some(None)`; no new dependency), and its utoipa schema is a nullable, non-required date-time. After regenerating, check that `mobile/lib/api/` can send an explicit `null` for `expires_at` (some Dart generators drop null fields); if it cannot, fix the generator options in `xtask` rather than changing the contract.
  - The POST body has no `all_accounts` field (absent `account_ids` means all accounts); the DTO uses `#[serde(deny_unknown_fields)]` so a stray `all_accounts` on POST is 422 rather than silently ignored.
  - `DELETE /api/v1/delegations/{id}` → 204.
  - `GET /api/v1/delegations/received?page…` → `Page<DelegationDto>`; `DELETE /api/v1/delegations/received/{id}` → 204. Both take `ActiveUser`.
  - `GET /api/v1/me/workspaces` → `Vec<WorkspaceDto { owner: UserRef, own: bool, delegation_id?, level?, all_accounts?, account_ids? }>`, takes `ActiveUser`.
  - `GET /api/v1/users/lookup?email=` → `{ user: { id, display_name } | null }`, takes `ActiveUser`, charges `state.limits.lookup` per call (429 `rate_limited` with `Retry-After`), never logs the email.
  - Each route's utoipa description says whether it accepts `X-Postit-Act-As` (only the `/delegations` owner routes take `Scope`, and they refuse delegates with `owner_only`).

- [ ] **Step 1: Write failing tests**: `grant_list_change_revoke_round_trip`; `change_with_both_account_fields_is_validation_failed` (PATCH with `all_accounts: true` and `account_ids` → 422 `field: "account_ids"`); `change_with_all_accounts_false_is_validation_failed`; `change_expiry_absent_null_and_set` (absent keeps it, `null` clears it, a future timestamp sets it); `grant_with_all_accounts_field_is_validation_failed` (POST); `grant_twice_is_delegation_exists`; `delegate_cannot_reach_delegation_routes_through_act_as` (`owner_only`); `delegate_lists_and_leaves_received_grant` (after leaving, the header is `delegation_invalid`); `workspaces_lists_own_first_then_received`; `lookup_matches_exactly_and_hides_ambiguity` (exact match, uppercase input, `"  email  "`, partial email → null, unverified → null, own email → null, duplicate verified email → null, all 200); `lookup_blank_or_missing_email_is_validation_failed` (`email=`, `email=%20%20`, and no `email` parameter → 422 `validation_failed`); `lookup_is_rate_limited` (with `rate_limit.lookup` set to 1 per minute, burst 1: second call 429); `delegation_responses_leak_no_secrets` (run the shared no-secret-leak harness over the responses above).
- [ ] **Step 2: Run** `cargo test -p postit-api --test delegations -- --test-threads=3` — FAIL.
- [ ] **Step 3: Implement**, including `AppState` construction in `postit-server` and the `TestApp`.
- [ ] **Step 4: Run** `cargo test -p postit-api -- --test-threads=3` and `cargo check -p postit-server` — PASS.
- [ ] **Step 5: Commit** `feat(api): add the delegation, workspace, and user lookup routes`.

### Task 8: Delegated access proven end to end

**Files:**
- Test: `backend/crates/api/tests/act_as.rs` (extend), `backend/crates/identity/tests/delegated_access.rs` (new)
- Modify: any `postit-data` repository whose capability disagrees with plan 01's table (expected: none; fix in place if found)

**Interfaces:**
- Consumes: everything above. No new production interfaces unless the audit finds a mismatch.

- [ ] **Step 1: Write the HTTP matrix** in `act_as.rs`, with grants made through `POST /delegations`: for owner, `view`, `edit`, `publish`, a `publish` delegate limited to account B, and an unrelated user acting as the owner (→ `delegation_invalid`): `GET /accounts` (limited sees only B), `GET /accounts/{A}` (limited → 404), `PATCH /accounts/{B}` and `POST /accounts/{B}/check` and `POST /accounts/credentials/mock` (every delegate → `owner_only`), `GET /media` (every delegate 200), `POST /media/uploads` (`view` → `capability_required`, `edit`+ → 201), `DELETE /media/{id}` (same). Then: `revocation_takes_effect_on_next_request`; `level_change_takes_effect_on_next_request` (`edit` downgraded to `view` → next upload `capability_required`); `disabled_owner_or_delegate_blocks_act_as` (admin disables one, header → `delegation_invalid`; re-enable restores); `disconnected_last_account_leaves_limited_grant_with_media_only` (Review Focus 4).
- [ ] **Step 2: Write the service-layer whole-object tests** in `delegated_access.rs`, using scopes from `resolve_act_as` over real grants and the B2 repositories directly: `edit_delegate_refused_deleting_post_with_live_history_allowed_on_draft`; `limited_delegate_refused_editing_post_with_override_outside_limit` (`AccountNotInScope`); `limited_delegate_refused_cancelling_job_with_delivery_outside_limit` and allowed when every non-terminal delivery is in the limit; `limited_delegate_reschedule_and_recurrence_change_follow_whole_object_rule`; `concurrent_owner_and_delegate_edits_one_gets_version_conflict`; `jobs_scheduled_by_delegate_survive_revocation` (job stays, `created_by` is the delegate).
- [ ] **Step 3: Audit capabilities**: for each repository in `backend/crates/data/src/` (`target_groups`, `writing_styles`, `campaigns`, `contacts`, `brand_profiles`, `recurrences`, `posts`, `publishing/*`, `media`, `accounts`), compare each method's `require(...)` with plan 01's level table and the REST surface's Access column (contact import and export and suppression removal `OwnerOnly`; recurrence changes `Publish`). There is no inbox repository yet (it arrives with the inbox phase), so inbox marks are not audited here. Write down each mismatch found in the commit body and fix it with a `scope_matrix.rs` case.
- [ ] **Step 4: Run** `cargo test -p postit-api -p postit-identity -p postit-data -- --test-threads=3` — PASS (the new tests should pass without production changes unless Step 3 found a mismatch; a failing one is a real gap: fix it, do not weaken the test).
- [ ] **Step 5: Commit** `test: prove delegated access at the service and HTTP layers`.

### Task 9: Contract, docs, deferred file, and gates

**Files:**
- Modify: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/` (all generated), `docs/security.md` (delegation threat entries: act-as resolution, the signed-URL window after a grant ends with the TTLs `media.view_url_ttl` 10 minutes and `media.public_url_ttl` 1 hour, lookup enumeration), `CLAUDE.md` (project state: B6 built)
- Create: `docs/superpowers/plans/2026-10-26-b6-delegation-deferred.md`

- [ ] **Step 1: Regenerate** with `cargo xtask openapi` from `backend/` (needs `pnpm install` in `web/` and `flutter pub get` in `mobile/`); check the new routes and both new error codes appear.
- [ ] **Step 2: Write the docs** listed above. Write the deferred file with one entry: "Notify both parties when a limited grant loses its last account, and the other party on every grant, change, revocation, leave, and expiry. Why not now: in-app notifications need `postit-notify`, which D1 creates. Trigger: plan 03 D1." Add any other issue found during execution that truly cannot be fixed in B6, each with its trigger.
- [ ] **Step 3: Run every gate** from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3`, `cargo xtask openapi --check`; from `web/`: `pnpm typecheck && pnpm lint && pnpm test` (through PowerShell); from `mobile/`: `flutter analyze`. All pass.
- [ ] **Step 4: Commit** `docs: document delegation and regenerate the API contract`.
