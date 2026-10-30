# B2a: Domain Schema, Scope, and Owned Repositories Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the whole plan 03 owned-content schema, delegated `OwnerScope` access with account limits, and the scoped repositories for accounts, media, posts, campaigns, target groups, writing styles, the brand profile, recurrences, and contacts. Prove isolation, same-owner references, and sealed-column hygiene by test.

**Architecture:**
- `postit-core` gains the remaining ID newtypes and canonical text forms for the enums that become columns.
- `postit-data` gains these parts:
  - `Access::Delegated` and one account-filter helper.
  - Every owned, connection, and deployment table in `0001_init.sql`, using composite `(owner_id, id)` foreign keys and `postit:sealed` column comments that a test checks against `SEALED_COLUMNS`.
  - A testkit helper that runs the four-caller isolation matrix.
  - One repository module per area.
- Posts are written only through a `LockedPost` token, which bumps the version and pauses the post's recurrence.

**Tech Stack:** Rust 2024, sqlx 0.9 (compile-checked macros, committed offline cache), PostgreSQL 18, `emixdb` pagination, `postit-vault`.

**Spec:** `docs/specs/2026-10-19-b2-domain-schema-design.md`, which argues from `docs/plans/03. publishing roadmap.md` phase B2 and plan 01's data model. Read both. B2b (`docs/plans/2026-10-19-b2b-publishing-concurrency.md`) follows on the same branch.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints. Tests may `#[allow]` the unwrap, expect, and panic lints locally, as existing tests do.
- `postit-data` depends on `config`, `core`, and `vault` only.
- Single migration: all schema goes into `backend/crates/data/migrations/0001_init.sql`. Reset the dev database after editing it.
- Queries use `sqlx::query!`, `query_as!`, and `query_scalar!`. After each task that adds queries, run `cargo sqlx prepare --workspace` from `backend/` against the dev Postgres, and commit `.sqlx` changes with the task.
- Owned-table columns: `id UUID PRIMARY KEY`, `owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE`, `created_at`/`updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`, `created_by`/`updated_by UUID REFERENCES users(id) ON DELETE SET NULL`, and `UNIQUE (owner_id, id)`. Owned-to-owned references are composite `(owner_id, x_id)` FKs. Nullable ones use `ON DELETE SET NULL (x_id)`.
- Every repository method on owned rows takes `&OwnerScope`, filters on `scope.owner`, writes `created_by`/`updated_by` from `scope.actor`, and applies `scope.account_filter()` to every account-bound row. A row outside the scope is `DataError::NotFound`, never a distinct error.
- Lists take `emixdb::Pagination` and return `emixdb::ResultSet<T>`, like the existing list methods.
- Database tests use `#[sqlx::test]`. Each repository gets an isolation test through the Task 2 helper.
- No plaintext secret in any column. No secret, email address, or content in any `DataError` message.
- After every task, run from `backend/`:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test -p postit-data` (plus any crate the task touched)

  Task 11 runs the full gates.
- Commits use Conventional Commits (`feat(data): …`, `feat(core): …`) and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Writing: American English.

## Review Focus

1. **A delegate whose account limit is empty.** Every account-bound list returns nothing, and every account-bound get is `NotFound`. Posts and media stay visible (plan 01, "Empty account limit"). Task 2 tests the helper. Tasks 6 and 7 test it on real queries.
2. **A limited delegate listing a post's overrides or targets.** Only rows for accounts in the limit come back, and `outside_limit` reports `true` when any hidden row exists. Task 7 tests this.
3. **A contact imported with different case or surrounding whitespace** (`" Bob@Example.COM "`). It matches the existing contact and the suppression hash for `bob@example.com`. Task 10 tests this.
4. **Reconnecting a remote account that a different owner disconnected.** It succeeds as a new row for the new owner, and the old owner's disconnected row and its delivery history stay. Task 6 tests this.
5. **A post edit with the right version while a dry-run job is still running.** It returns `PostLocked`, not success. Task 7 tests this.

---

### Task 1: Core IDs and enum text forms

**Files:**
- Modify: `backend/crates/core/src/ids.rs`, `backend/crates/core/src/lib.rs`, `backend/crates/core/src/delivery.rs`, `backend/crates/core/src/post.rs`
- Test: inline `#[cfg(test)]` modules in the same files

**Interfaces:**
- Produces:
  - ID newtypes, made with the existing macro: `GrantId`, `TargetGroupId`, `RecurrenceId`, `ContactId`, `ContactListId`, `ContactImportId`, `DeliveryAttemptId`, `InboxItemId`, `AiProviderId`, `AiGenerationId`, `NotificationId`, `PushEndpointId`, `DelegationId`, `WritingStyleId`, `OAuthStateId`, `PendingConnectionId`. All are re-exported from the crate root.
  - `DeliveryState::as_str(&self) -> &'static str` and `DeliveryState::from_parts(state: &str, parked_reason: Option<&str>) -> Result<Self, ParseEnumError>`. `Parked(r)` has the state string `parked`, and `ParkReason::as_str` gives the reason.
  - Strings, snake_case as in plan 03:
    - `pending`, `claimed`, `in_flight`, `processing`, `outcome_unknown`, `parked`, `published`, `failed`, `dead_letter`, `cancelled`, `simulated`.
    - Reasons: `owner_disabled`, `account_needs_reconnect`, `platform_unavailable`, `platform_disabled`, `live_publish_disabled`, `quota_exceeded`, `outcome_unresolvable`.
  - `JobStatus::as_str`/`FromStr`: `scheduled`, `publishing`, `published`, `cancelled`, `failed`, `partial`.
  - `PostKind::as_str`/`FromStr`: `post`, `comment`.
  - A core `ParseEnumError` that carries the rejected value's length only. If core already has an equivalent, reuse it.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn every_delivery_state_round_trips() // iterate all 11 states x every ParkReason: from_parts(as_str, reason) == original
#[test] fn parked_without_reason_is_an_error() // from_parts("parked", None) -> Err
#[test] fn unknown_state_is_an_error()
#[test] fn job_status_and_post_kind_round_trip()
```

- [ ] **Step 2: Run** `cargo test -p postit-core`. Expected: FAIL (the compile fails).
- [ ] **Step 3: Implement.** The new IDs go in alphabetical blocks next to the existing ones.
- [ ] **Step 4: Run** `cargo test -p postit-core`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add plan 03 ID newtypes and canonical enum strings`.

---

### Task 2: Delegated access, the account filter, and the isolation helper

**Files:**
- Modify: `backend/crates/data/src/scope.rs`, `backend/crates/data/src/lib.rs`, and every caller that relied on `OwnerScope: Copy` (`rg "OwnerScope" backend/crates`)
- Create: `backend/crates/data/src/testkit.rs` (behind the existing `testkit` feature)
- Test: `backend/crates/data/src/scope.rs` (unit), `backend/crates/data/tests/scope_matrix.rs`

**Interfaces:**
- Produces:
  - `pub enum Level { View, Edit, Publish }` (ordered).
  - `pub enum AccountLimit { All, Only(Arc<[AccountId]>) }` with `AccountLimit::only(ids: impl IntoIterator<Item = AccountId>) -> Self`, which sorts and deduplicates.
  - `pub enum Access { Owner, Delegated { delegation: DelegationId, level: Level, accounts: AccountLimit } }`
  - `pub enum Capability { View, Edit, Publish, OwnerOnly }`. `ScopeError` gains `OwnerOnly`.
  - `OwnerScope` becomes `#[derive(Debug, Clone)]` (no `Copy`). It adds `fn delegated(actor, owner, delegation, level, accounts) -> Self`, `fn account_filter(&self) -> Option<Vec<Uuid>>`, and `fn allows_account(&self, AccountId) -> bool`.
  - `account_filter` returns `None` for `Owner` and for `AccountLimit::All`, and `Some(ids)` otherwise. `Some(vec![])` is the empty limit.
  - Testkit:
    - `pub async fn seed_user(pool, status: &str) -> UserId`, which inserts directly with a UUID v7.
    - `pub struct Callers { pub owner: OwnerScope, pub view: OwnerScope, pub edit: OwnerScope, pub publish: OwnerScope, pub limited: OwnerScope /* publish level, limited to `other_accounts` */, pub stranger: OwnerScope }`.
    - `pub async fn callers(pool, owner: UserId, other_accounts: &[AccountId]) -> Callers`.
  - `Callers` is plain data. Each repository test asserts its own expectations per caller.

- [ ] **Step 1: Write failing unit tests**

```rust
#[test] fn delegated_levels_gate_capabilities() // View: View ok, Edit Err(InsufficientAccess); Edit: Edit ok, Publish Err; Publish: Publish ok
#[test] fn owner_only_refused_for_every_delegate() // all three levels -> Err(OwnerOnly); Owner -> Ok
#[test] fn account_filter_shapes() // Owner None; All None; Only([b,a,a]) Some([a,b]); Only([]) Some([])
#[test] fn allows_account_respects_limit()
```

  Also add `tests/scope_matrix.rs`. It runs `SELECT count(*) FROM (VALUES ($1::uuid)) v(a) WHERE ($2::uuid[] IS NULL OR a = ANY($2))` with each caller's filter and checks the counts: 1 for owner/All and 0 for an empty `Only`. This proves the SQL idiom binds correctly through sqlx.

- [ ] **Step 2: Run** `cargo test -p postit-data scope`. Expected: FAIL.
- [ ] **Step 3: Implement.** Fix the callers the compiler reports by borrowing or cloning.
- [ ] **Step 4: Run** `cargo test -p postit-data` and `cargo check --workspace --all-targets`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add delegated access with account limits to OwnerScope`.

---

### Task 3: The schema, the sealed catalog, and schema-level tests

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/vault.rs` (`SEALED_COLUMNS`), `backend/crates/config/src/settings.rs` (`RetentionSettings::contact_imports`, default `30days`), `backend/config/default.toml`, `docs/configuration.md`
- Test: `backend/crates/data/tests/schema.rs`

**Interfaces:**
- Produces these tables, with the columns from plan 01 "Owned content" and "Deployment" plus the spec's additions:
  - `account_grants`, `connected_accounts`, `account_tokens`, `oauth_states`, `pending_connections`
  - `writing_styles`, `brand_profiles`, `target_groups`, `target_group_accounts`, `campaigns`
  - `media_assets`, `posts`, `post_media`, `post_targets`, `post_overrides`, `recurrences`, `recurrence_targets`
  - `publish_jobs`, `deliveries`, `delivery_attempts`
  - `contacts`, `contact_lists`, `contact_list_members`, `suppressions`, `contact_imports`
  - `direct_sends`, `email_events`, `inbox_items`, `post_metric_snapshots`, `account_metrics_daily`
  - `ai_providers`, `ai_generations`, `platform_quota_usage`, `push_endpoints`, `notifications`
  - `app_settings`, `user_limits`, `platform_app_quota_usage`
- `delegations` and `delegation_accounts` are not created; they come in B6.
- The spec's additions to plan 01's columns:
  - `contacts.email_hash BYTEA NOT NULL`
  - `deliveries.post_id` (composite FK) and `deliveries.parked_notified_at`
  - `direct_sends.lease_expires_at`
  - `'ended'` in the `recurrences.paused_reason` CHECK
  - `publish_jobs.status` CHECK on the `JobStatus` strings
- `deliveries.state` CHECK on the Task 1 state strings, plus `CHECK ((state = 'parked') = (parked_reason IS NOT NULL))`.
- The named constraints and indexes from the spec's "Indexes and constraints" section. Name each unique constraint explicitly (`deliveries_idempotency_key_key`, `publish_jobs_one_live_job_idx`, `connected_accounts_remote_key_idx`, `connected_accounts_one_email_idx`, `contacts_owner_email_key`, `suppressions_owner_hash_key`, `direct_sends_delivery_contact_key`). Task 4 translates by these names.
- `COMMENT ON COLUMN … IS 'postit:sealed'` on the nine sealed columns the spec lists, and nine matching `SealedColumn` entries.
- `oauth_states.user_id` and `push_endpoints.user_id` and `notifications.user_id` use `ON DELETE CASCADE` (recipient columns).

- [ ] **Step 1: Write failing tests** in `tests/schema.rs`:

```rust
#[sqlx::test] async fn sealed_comments_match_catalog() // SELECT table,column FROM pg_description join pg_attribute where description='postit:sealed' == SEALED_COLUMNS set
#[sqlx::test] async fn sealed_columns_are_bytea()
#[sqlx::test] async fn every_owned_table_has_owner_and_actor_columns() // list of owned table names from plan 01 (minus delegations): owner_id NOT NULL, created_by/updated_by nullable, unique (owner_id,id) exists
#[sqlx::test] async fn delivery_state_check_accepts_every_core_state() // insert a minimal job+delivery per DeliveryState via raw SQL inside a rolled-back tx; every core state accepted, "bogus" rejected, parked without reason rejected
#[sqlx::test] async fn composite_fks_reject_cross_owner_refs() // table-driven: post_media->other owner's media, post_overrides->other owner's account, connected_accounts->other owner's grant, contact_list_members->other owner's contact, target_group_accounts->other owner's account: each insert fails with a foreign_key_violation
#[sqlx::test] async fn nullable_composite_fk_keeps_owner_on_parent_delete() // delete campaign: posts.campaign_id NULL, posts.owner_id unchanged
#[sqlx::test] async fn plaintext_never_stored() // seal fixture secrets with a test Vault into every catalog column (using each row's own SealContext), then for every text/jsonb/bytea column of every public table: SELECT count(*) WHERE col::text LIKE '%'||fixture||'%' (bytea via encode(col,'escape')) == 0 for each fixture
```

  For the plaintext check, the fixture secrets are long, distinctive ASCII strings (`postit-fixture-secret-…`). Build a `Vault` with the development key from `postit-config`.

- [ ] **Step 2: Run** `cargo test -p postit-data --test schema`. Expected: FAIL.
- [ ] **Step 3: Implement** the schema. Reset the dev database. Add `retention.contact_imports` to the config, `default.toml`, and the docs table.
- [ ] **Step 4: Run** `cargo test -p postit-data` and `cargo test -p postit-config`. Expected: PASS. Then run `cargo test --workspace` once to catch fixtures that build `RetentionSettings` literally.
- [ ] **Step 5: Commit** `feat(data): add the plan 03 owned-content schema with sealed-column comments`.

---

### Task 4: Errors, constraint translation, user limits, and app settings

**Files:**
- Modify: `backend/crates/data/src/error.rs`, `backend/crates/data/src/users.rs` (`provision` and the bootstrap insert), `backend/crates/identity/src/` (callers of `provision`), `backend/crates/server/src/lib.rs` (seed app settings at API startup, next to the platform upsert)
- Create: `backend/crates/data/src/limits.rs`, `backend/crates/data/src/app_settings.rs`, `backend/crates/data/src/constraint.rs`
- Test: `backend/crates/data/tests/limits.rs`, `backend/crates/data/tests/app_settings.rs`, `backend/crates/data/tests/users.rs` (extend)

**Interfaces:**
- Produces:
  - `DataError` gains `VersionConflict`, `PostLocked`, `JobActive`, `AccountConnectedElsewhere`, `SendingMethodExists`, `AccountNotInScope`, and `Scope(#[from] ScopeError)`.
  - `pub(crate) fn translate(err: sqlx::Error) -> DataError` maps by constraint name:
    - `publish_jobs_one_live_job_idx` to `JobActive`
    - `connected_accounts_remote_key_idx` to `AccountConnectedElsewhere`
    - `connected_accounts_one_email_idx` to `SendingMethodExists`
    - other unique violations to `Conflict(constraint name)`
    - everything else to `Sql`
  - `#[derive(Clone, Copy, Debug)] pub struct UserLimitDefaults { pub email_daily_cap: u32, pub contacts_cap: u32, pub storage_quota_bytes: u64 }` with `from_settings(&Settings) -> Self`. `storage_quota_bytes` falls back to `media.user_quota_bytes`.
  - `UsersRepo::provision(conn, id, issuer, subject, display_name, limits: UserLimitDefaults)` inserts `user_limits` in the same statement sequence when it creates the user. The bootstrap path does the same.
  - `UserLimitsRepo::get(conn, UserId) -> Result<UserLimits, DataError>` and `UserLimitsRepo::update(conn, UserId, UserLimits, admin: UserId)`. `UserLimits` mirrors the columns.
  - `AppSettingsRepo::seed_defaults(conn, live: bool)` inserts `publishing.live` if it is absent and never overwrites. It also has `live_publishing(conn) -> Result<bool, DataError>` and `set_live_publishing(conn, bool, admin: UserId)`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn provision_creates_limits_from_defaults() // defaults {500, 10000, 21474836480} -> get() equal
#[sqlx::test] async fn provision_twice_keeps_first_limits()
#[sqlx::test] async fn limits_update_records_admin()
#[sqlx::test] async fn seed_does_not_overwrite() // seed(true); set(false); seed(true) -> live_publishing()==false
#[sqlx::test] async fn translate_maps_named_constraints() // provoke the email one-account index via raw inserts -> SendingMethodExists
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Thread `UserLimitDefaults` through `postit-identity`'s provisioning. Build it once from settings in `postit-server`. Seed app settings in the API process only.
- [ ] **Step 4: Run** `cargo test -p postit-data -p postit-identity -p postit-server`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add user limits, app settings, and constraint translation`.

---

### Task 5: `IdempotencyRepo` takes `&OwnerScope`

**Files:**
- Modify: `backend/crates/data/src/idempotency.rs`, `backend/crates/api/src/idempotency.rs`
- Test: `backend/crates/data/tests/idempotency.rs`, `backend/crates/api` tests that cover idempotency (update the call sites)

**Interfaces:**
- Produces: every `IdempotencyRepo` method that took `owner_id` and `actor_id` now takes `scope: &OwnerScope` and reads `scope.owner` and `scope.actor`. Methods that act by row `id` (`complete`, `release`, `delete`) keep their signatures. The API middleware passes the request's resolved scope (today `OwnerScope::own(principal.user_id)`; B6 makes it the act-as scope).

- [ ] **Step 1: Update the existing tests** to construct scopes, and add `delegate_and_owner_keys_do_not_collide`: the same key used by an owner scope and by a delegated scope for the same owner gives two separate records, because the unique key is `(owner_id, actor_id, key)`.
- [ ] **Step 2: Run** the test. Expected: FAIL (the compile fails).
- [ ] **Step 3: Implement** the signature change and fix the callers.
- [ ] **Step 4: Run** `cargo test -p postit-data -p postit-api`. Expected: PASS.
- [ ] **Step 5: Commit** `refactor(data): take OwnerScope in IdempotencyRepo`. In the body, note that it closes P4 deferred ruling 6.

---

### Task 6: Accounts repository

**Files:**
- Create: `backend/crates/data/src/accounts.rs`
- Test: `backend/crates/data/tests/accounts.rs`

**Interfaces:**
- Consumes: Task 2 scope and testkit, Task 4 `translate`, `postit_vault::{Vault, Sealed, SealContext}`.
- Produces (all take `conn: &mut PgConnection` first and `scope: &OwnerScope` second):
  - `GrantRepo`:
    - `insert(NewGrant { id: GrantId, platform: PlatformId, kind: GrantKind, remote_user_id: Option<String>, credential: Sealed<serde_json::Value>, connection_config: Value, expires_at, reconnect_by, scopes: Vec<String> }) -> Grant`
    - `get(GrantId) -> Grant`
    - `set_status(GrantId, GrantStatus) -> ()`, which sets every account on the grant to the same status, except `disconnected` accounts
    - `account_count(GrantId) -> i64`
    - `delete_if_unused(GrantId) -> bool`

    Grants require `Capability::OwnerOnly` for writes and `View` for `get`. `Grant` exposes the sealed credential only as `Sealed<Value>`, never opened here.
  - `AccountRepo`:
    - `insert_or_reactivate(NewAccount { id, grant_id, platform, remote_account_id, display_name, avatar_url, scopes, test_account }) -> AccountUpsert`. `AccountUpsert` is `Created(Account)`, `Reactivated(Account)`, or `Moved(Account)` (an existing row of this owner moved to a new grant). A row owned by someone else that is not `disconnected` gives `Err(AccountConnectedElsewhere)`.
    - `get(AccountId) -> Account` (account filter applies)
    - `list(Pagination, platform: Option<&PlatformId>) -> ResultSet<Account>` (filtered)
    - `set_status(AccountId, AccountStatus)`
    - `mark_checked(AccountId, at)`
    - `disconnect(AccountId) -> Disconnected { grant_now_unused: bool, paused_recurrences: Vec<RecurrenceId> }`. Disconnect is one transaction. It sets `disconnected`, nulls `grant_id`, deletes `account_tokens`, `post_overrides`, `post_targets`, and `inbox_items` for the account, removes it from `target_group_accounts` and `recurrence_targets`, and pauses recurrences left with no targets (`paused_reason = 'no_targets'`). The cancellation of pending and parked deliveries for the account is B2b's `cancel_for_account`; this method's rustdoc says the service calls both in one transaction.

    Writes require `OwnerOnly`; reads require `View`.
  - `AccountTokenRepo`: `upsert(AccountId, Sealed<Value>, expires_at)` and `get(AccountId) -> Option<(Sealed<Value>, Option<DateTime>)>`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn isolation_matrix() // seed owner with accounts A (in limit) and B; callers(limited to [A]): owner/view/edit/publish see A,B; limited sees A only (get(B) NotFound); stranger sees none; writes: delegates get Err(Scope(OwnerOnly))
#[sqlx::test] async fn empty_limit_sees_no_accounts() // limited to [] -> list empty, get(A) NotFound
#[sqlx::test] async fn connected_elsewhere_hides_owner() // owner2 connects same (platform, remote) -> Err(AccountConnectedElsewhere); error Display contains no uuid of owner1
#[sqlx::test] async fn disconnected_remote_can_be_taken_by_another_owner() // owner1 disconnects; owner2 inserts -> Created; owner1 row still exists, status disconnected
#[sqlx::test] async fn same_owner_reconnect_reactivates() // disconnect then insert again by owner1 -> Reactivated, same AccountId
#[sqlx::test] async fn one_email_account_per_owner() // second non-disconnected email account -> SendingMethodExists
#[sqlx::test] async fn grant_status_spreads_to_accounts() // set_status(NeedsReconnect) -> both accounts needs_reconnect
#[sqlx::test] async fn disconnect_cleans_links_and_pauses_empty_recurrence() // account in a target group, recurrence with only this target, an override -> all removed; recurrence paused 'no_targets'; returned in paused_recurrences
#[sqlx::test] async fn grant_shared_by_two_accounts_reports_unused_only_after_last() 
```

  Seed recurrences and overrides with raw SQL in this test, because their repositories arrive in Tasks 7 and 9.

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** `GrantKind`, `GrantStatus`, and `AccountStatus` are enums with `as_str`/`FromStr` in this module, matching the CHECKs.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add grant and connected-account repositories`.

---

### Task 7: Media and posts repositories with the post lock

**Files:**
- Create: `backend/crates/data/src/media.rs`, `backend/crates/data/src/posts.rs`
- Test: `backend/crates/data/tests/media.rs`, `backend/crates/data/tests/posts.rs`

**Interfaces:**
- Produces:
  - `MediaRepo`:
    - `insert(NewMedia { id, storage_key, original_filename, mime, bytes, status, declared_bytes, upload_expires_at, derived_from, fit_spec })`
    - `get(MediaAssetId)`
    - `list(Pagination)`
    - `set_ready(MediaAssetId, ReadyMedia { bytes, sha256, width, height, duration_ms, thumbnail_key })`
    - `delete(MediaAssetId) -> Result<(), DataError>`. It returns `Conflict("media_in_use")` while a `post_media` row references the asset. The snapshot rule is B2b's.
    - `stored_bytes(UserId) -> i64`. This is a system read: ready bytes plus the `declared_bytes` of uploading rows.

    Writes require `Edit`.
  - `PostRepo`:
    - `create(NewPost { id, kind: PostKind, reply_to: Option<Value>, campaign_id, text, link, tags, disclose_ai_generated }) -> Post` with version 1 and status `draft`. It requires `Edit`.
    - `get(PostId) -> Post`
    - `list(Pagination, PostFilter { status, campaign_id, kind }) -> ResultSet<Post>`. Posts are visible whole to limited delegates.
    - `lock(PostId, expected_version: Option<i64>) -> LockedPost`. It runs `SELECT … FOR UPDATE`. `VersionConflict` comes before `PostLocked`. `PostLocked` means any publish job of the post whose status is `scheduled` or `publishing`, dry-run included.
    - Through `&LockedPost`, every write bumps the version: `update_content(&LockedPost, PostContent)`, `set_media(&LockedPost, &[PostMediaItem { media_id, position, alt_text }])`, `set_targets(&LockedPost, &[AccountId])` (it replaces only targets inside the scope's limit and keeps the others), `upsert_override(&LockedPost, AccountId, patch: Value, options: Value)`, `delete_override(&LockedPost, AccountId)`, and `delete(LockedPost)`.
    - `delete` requires `Publish` when the post has any live publish job, else `Edit`.
    - `targets(PostId)` and `overrides(PostId)` apply the account filter.
    - `outside_limit(PostId) -> bool` checks overrides, targets, and non-terminal deliveries.
  - `pub struct LockedPost { post: Post, scope: OwnerScope }`. The fields are private and the only constructor is `lock`. It has `post(&self) -> &Post`. The private `bump_version(conn, &LockedPost)` increments `version`, sets `updated_by`, and runs `UPDATE recurrences SET paused = true, paused_reason = 'post_edited' WHERE post_id = $1 AND NOT paused`.
  - Every write above calls `bump_version` exactly once. Account-bound writes (targets and overrides) check `scope.allows_account` and return `NotFound` otherwise.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn media_isolation_matrix() // view reads, edit writes, stranger NotFound
#[sqlx::test] async fn media_in_use_cannot_be_deleted()
#[sqlx::test] async fn stored_bytes_counts_ready_and_open_sessions()
#[sqlx::test] async fn post_isolation_matrix() // view: get ok, create Err(InsufficientAccess); edit: create/update ok; limited: sees post, sees only its own overrides/targets; stranger NotFound
#[sqlx::test] async fn every_write_bumps_version_once() // create v1; update_content v2; set_media v3; set_targets v4; upsert_override v5; delete_override v6
#[sqlx::test] async fn stale_version_conflicts() // lock(Some(1)) after bump -> VersionConflict
#[sqlx::test] async fn running_dry_run_locks_post() // raw-insert a dry-run job status 'publishing' -> lock(Some(v)) Err(PostLocked)
#[sqlx::test] async fn edit_pauses_recurrence() // raw-insert recurrence; update_content -> paused, reason post_edited
#[sqlx::test] async fn limited_set_targets_keeps_hidden_targets() // owner targets [A,B]; limited([A]) set_targets([]) -> owner sees [B]
#[sqlx::test] async fn outside_limit_detects_hidden_override() // owner override on B; limited([A]).outside_limit -> true; owner -> false
#[sqlx::test] async fn delete_with_live_history_needs_publish() // raw-insert terminal live job; edit delegate delete -> Err(InsufficientAccess); publish delegate -> Ok
#[sqlx::test] async fn comment_post_round_trips_reply_to()
#[sqlx::test] async fn concurrent_lock_waits() // tx1 lock & hold; tx2 lock in spawned task blocks until tx1 commits a bump, then gets VersionConflict for the old version
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** `PostContent`, `PostFilter`, and `PostMediaItem` are plain structs in `posts.rs`.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add media and post repositories with the post lock and versions`.

---

### Task 8: Campaigns, target groups, writing styles, and the brand profile

**Files:**
- Create: `backend/crates/data/src/campaigns.rs`, `backend/crates/data/src/target_groups.rs`, `backend/crates/data/src/writing_styles.rs`, `backend/crates/data/src/brand_profiles.rs`
- Test: one test file per module

**Interfaces:**
- Produces:
  - `CampaignRepo`: `create`, `get`, `list(Pagination, include_archived: bool)`, `update`, `archive`, `delete`. Deleting a campaign nulls `posts.campaign_id` (FK). Writes require `Edit`.
  - `TargetGroupRepo`: `create(name, accounts)`, `get`, `list`, `rename`, `set_accounts`, `delete`. Members are filtered by the account limit when read. A limited delegate's `set_accounts` keeps hidden members, as with post targets.
  - `WritingStyleRepo`: `upsert(AccountId, WritingStyle)`, `get(AccountId)`, `list()`. All are account-bound and filtered.
  - `BrandProfileRepo`: `get() -> Option<BrandProfile>` and `upsert(BrandProfile)`, one per owner. `examples` holds at most 5 entries, enforced by a CHECK `cardinality(examples) <= 5` (add the CHECK to the schema if Task 3 did not).

- [ ] **Step 1: Write failing tests:** an isolation matrix per repository, then:

```rust
#[sqlx::test] async fn campaign_delete_detaches_posts()
#[sqlx::test] async fn limited_delegate_target_group_keeps_hidden_members()
#[sqlx::test] async fn writing_style_outside_limit_is_not_found()
#[sqlx::test] async fn brand_profile_rejects_six_examples()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add campaign, target group, writing style, and brand profile repositories`.

---

### Task 9: Recurrences (CRUD)

**Files:**
- Create: `backend/crates/data/src/recurrences.rs`
- Test: `backend/crates/data/tests/recurrences.rs`

**Interfaces:**
- Consumes: `LockedPost` from Task 7. `bump_version` must be `pub(crate)` so recurrence writes reuse it.
- Produces:
  - `RecurrenceRepo::set(&LockedPost, NewRecurrence { id, rule: String, timezone: String, targets: Vec<AccountId>, rewrite: bool, next_occurrence_at, confirmed_version: i64 }) -> Recurrence`. It records `confirmed_by = actor` and `confirmed_at`, and replaces any existing recurrence for the post. It requires `Publish`.
  - `get(PostId) -> Option<Recurrence>`. Targets are filtered by the limit.
  - `pause(&LockedPost, reason)` and `resume(&LockedPost, next_occurrence_at, confirmed_version)` both require `Publish`.
  - `delete(&LockedPost)`.
  - Setting or deleting the recurrence bumps the post version through `bump_version`. The recurrence row it writes must end unpaused for `set` and `resume`. So `set` and `resume` bump first, then write `paused = false`. Document this ordering in the rustdoc.
  - B2b adds the tick queries (`due`, `advance`, `record_skip`).

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn set_requires_publish() // edit delegate -> InsufficientAccess
#[sqlx::test] async fn set_bumps_post_version_and_is_active()
#[sqlx::test] async fn later_post_edit_pauses_it()
#[sqlx::test] async fn resume_unpauses_and_records_confirmation()
#[sqlx::test] async fn targets_from_other_owner_rejected() // composite FK -> error
#[sqlx::test] async fn limited_delegate_sees_only_in_limit_targets()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add the recurrence repository`.

---

### Task 10: Contacts, lists, suppressions, and imports

**Files:**
- Create: `backend/crates/data/src/contacts.rs`, `backend/crates/data/src/email_hash.rs`
- Test: `backend/crates/data/tests/contacts.rs`

**Interfaces:**
- Produces:
  - `pub fn normalize_email(raw: &str) -> String`: trim, then ASCII-lowercase the whole address. `pub fn email_hash(key: &[u8], raw: &str) -> [u8; 32]` is HMAC-SHA256 of `normalize_email(raw)`. This is the single owner of the hash. B2b and C1 use it.
  - `ContactRepo`:
    - `upsert(key: &[u8], NewContact { id, email, first_name, last_name, fields, tags, locale, consent_source, consent_at }) -> ContactUpsert` (`Created` or `Updated`). It matches on the normalized email and writes `email_hash`.
    - `get`, `list(Pagination, ContactFilter { list_id, tag, status })`, `set_status(ContactId, ContactStatus)`, and `delete(ContactId)`.
    - `count(UserId) -> i64` is a system read used by C1 against `contacts_cap`.
    - Writes require `Edit`.
  - `ContactListRepo`: `create`, `get`, `list`, `rename`, `delete`, `add_members(ListId, &[ContactId])`, and `remove_members`. Member rows repeat `owner_id`.
  - `SuppressionRepo`:
    - `add(key, email, reason)` and `add_hash(hash, reason)`. Both are idempotent per owner, and both set any matching contact's status: `unsubscribed`, `bounced`, or `complained`. `manual` leaves the contact's status unchanged.
    - `contains(hash) -> bool` and `list(Pagination)`. The list returns hashes and reasons only.
    - `remove(hash)` requires `OwnerOnly`.
  - `ContactImportRepo`: `create(rows_total, attested_by)`, `finish(ImportId, ok, rejected, report: Value)`, `get`, and `list`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn normalize_trims_and_lowercases() // " Bob@Example.COM " -> "bob@example.com"
#[sqlx::test] async fn upsert_matches_case_insensitively() // second upsert with " BOB@example.com" -> Updated, same id, one row
#[sqlx::test] async fn suppression_hash_matches_contact_hash()
#[sqlx::test] async fn unsubscribe_suppression_marks_contact()
#[sqlx::test] async fn suppression_survives_contact_delete()
#[sqlx::test] async fn removing_suppression_is_owner_only()
#[sqlx::test] async fn list_member_of_other_owner_rejected() // composite FK
#[sqlx::test] async fn contacts_isolation_matrix() // contacts are visible whole to limited delegates; stranger NotFound
#[sqlx::test] async fn suppression_list_exposes_no_address() // the row type has no email field (compile-level) and the stored row has no plaintext address
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Use the `hmac` and `sha2` crates that `postit-data` already depends on.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add contact, list, suppression, and import repositories`.

---

### Task 11: Full gates

- [ ] **Step 1: Run**, from `backend/`:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo check --workspace --all-targets`
  - `cargo test --workspace`
  - `cargo sqlx prepare --check --workspace` (needs the dev Postgres)

  Expected: all pass.
- [ ] **Step 2:** If `cargo xtask openapi --check` fails because an API type changed, regenerate it with `cargo xtask openapi` and commit the result. B2a should not change the API surface.
- [ ] **Step 3: Commit** any `.sqlx` stragglers with `chore(data): refresh the sqlx offline cache`.
