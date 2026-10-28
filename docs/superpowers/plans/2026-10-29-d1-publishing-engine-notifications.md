# D1: Publishing Engine, Notifications, and Push Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make queued publishing real (durable, safely claimed, observable, retryable, idempotent, never duplicated after a remote success) on the mock agents, and let postit tell people things: in-app notifications, coalesced critical emails, a daily comment digest, and Web Push to Android (UnifiedPush) and browsers.

**Architecture:**
- `postit-jobs` gains the `publishing` queue, re-queues unknown job types instead of killing them, and reports console disabled-action reasons as codes.
- `postit-core` gains the notification vocabulary (`NotificationKind`, `Severity`, `Notice` and its payload structs). `postit-data` gains the notification, push-endpoint, and preference queries; the delivery queries already exist (B2b, `data/src/publishing/`).
- New `postit-notify` (plan 01 level 6) records notifications inside the caller's transaction (`Notifier::record`), resolves recipients, and fans out to email (`MailContextLoader`s for a coalesced alert and the digest) and push (`push_send`, RFC 8291 + VAPID through `postit-http`).
- New `postit-publishing` (level 10) owns the publishing jobs, the worker flow, the due sweep, preflight, the direct flow, the service API D3 will wrap, the publishing and media deletion steps, and the disconnect hook `postit-oauth` calls.
- `postit-oauth` and `postit-identity` become notification producers (reconnect, revocation, `reconnect_by` stages, delegation changes, pending users). `postit-api` adds the notification, push, and widened preference routes. The web client gets a service worker, a CSP update, and the notification settings on the profile page.

**Tech Stack:** Rust 2024, axum 0.8, sqlx (Postgres), apalis behind `postit-jobs`, askama + Fluent (`postit-i18n`) for email and push text, `postit-http` for every outbound request, wiremock, utoipa; React 19 + Vite + Vitest in `web/`. New workspace dependencies: `chrono-tz` (MIT/Apache-2.0) and a Web Push crate chosen in Task 20 (expected `web-push` with `default-features = false`, MIT/Apache-2.0, which pulls `ece`, MPL-2.0; both licenses are in `backend/deny.toml`).

**Spec:** `docs/superpowers/specs/2026-10-29-d1-publishing-engine-notifications-design.md` (approved). It argues from `!ref/plans/03. publishing roadmap.md` (Phase D1, "Publishing", "Delivery states", "Rescheduling, cancelling, and retrying", "Quotas", "Disconnecting an account", "Sending an email campaign" steps 3–7, "Credential refresh", "Notifications and push", jobs and audit tables) and `!ref/plans/01. vision and architecture.md` (crate levels, delegation notification rules, `notifications`). Read all three first. The spec's "My call" items and the maintainer's decisions (one plan, browser push, labeled payloads) are not reopened.

## Global Constraints

- Branch `d1-publishing-notifications` from `stage`. At start, scan `docs/superpowers/plans/*-deferred.md` for entries whose trigger is D1 (at plan time: B4 second entry, B5, B6 first entry, B7; all are tasks below).
- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests).
- Single migration policy: edit `backend/crates/data/migrations/0001_init.sql` in place; reset the dev database after.
- Crate levels (plan 01): `notify` depends on config, core, http, data, jobs, mail, i18n, vault only. `identity` and `oauth` may depend on `notify`; `notify` never depends on them. `publishing` depends on core, data, jobs, notify, identity, oauth, media, agent-kit (not `contacts` or `drafter`). Lower crates reach higher ones only through traits `postit-server` injects (`UserDeletionStep`, the new `DisconnectHook`).
- `postit-http` is the only place that builds `reqwest` clients. Push goes through `postit_http::build_public_client` (address policy); `push.allow_private_endpoints` builds the permissive variant.
- `postit-jobs` is the only crate that depends on apalis. Publishing jobs: `Queue::Publishing`, `RetryPolicy::None`, console retry and cancel disabled with reason `publishing_managed`; the delivery state machine owns retries.
- Reuse the B2b data layer: `DeliveryRepo` (`claim`, `renew_lease`, `apply`, `set_container_ref`, `record_late_receipt`, `cancel`, `cancel_for_account`, `retry`, `resolve`, `refresh_job_status`, `account_has_in_flight`), `AttemptRepo`, `DirectRepo`, `QuotaRepo`, `PlatformAppQuotaRepo`, `Sweep`, `JobRepo`. Add queries there; do not duplicate one.
- Every write after a claim goes through a `Fence` and may come back `Applied::Stale`; a stale writer holding a success receipt calls `DeliveryRepo::record_late_receipt`, never `apply`. Every write that changes a delivery is followed by `DeliveryRepo::refresh_job_status` in the same transaction.
- Settings (exist, `[publishing]`): `live = true`, `max_attempts = 5`, `lease = "5min"` (renew every `lease / 3`), `sweep_grace = "2min"`, `park_grace = "15min"`, `processing_timeout = "24h"`. New: `[jobs] unknown_type_max_age = "7days"`, `[jobs.concurrency] publishing = 4`, `[jobs.schedules] delivery_due_sweep = "0 * * * * *"`, `account_preflight = "0 */10 * * * *"`, `notification_digest = "0 0 * * * *"`. `[push]` (exists): `enabled`, `vapid_private_key` (secret), `vapid_subject`, `allow_private_endpoints`.
- Backoff constants (spec leaves the values to the plan): retry `min(30min, 30s × 2^attempt)` with full jitter (uniform in `[0, that]`); processing polls `min(15min, 15s × 2^poll)`, no jitter; a `RetryAfter(d)` from an agent wins over the computed delay.
- Critical-email delay: `ALERT_DELAY = 2min`. Digest hour: `DIGEST_HOUR = 8` (local, `user_preferences.timezone`).
- Notification payloads carry IDs, counts, and the labels the spec names (account display name, platform ID, the other party's display name for delegation kinds). Never post or comment text, addresses, tokens, endpoint URLs, or keys, in payloads, emails, push bodies, audit details, logs, or job payloads. Endpoint URLs in logs only as `Sensitivity::Opaque`.
- Every new owned-data or self route gets the four-caller isolation tests (owner, delegate at each level, delegate limited to other accounts, unrelated user); the notification, push, and preference routes answer 403 `owner_only` under `X-Postit-Act-As`.
- New problem codes: `push_disabled` (404 on `GET /push/vapid-public-key` when push is off), `push_endpoint_invalid` (422). Both get web and mobile error-map entries in English and Arabic.
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=D:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from the crate that gained them. Web tasks also run the gates in `web/CLAUDE.md` through PowerShell with pnpm (never npm). A task that changes the API contract runs `cargo xtask openapi` and commits `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`.
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`. American English everywhere, including Arabic-catalog keys' English comments.

## Review Focus

1. **A worker stalls past its lease mid-publish, the sweep marks the delivery `OutcomeUnknown`, reconcile finds nothing and re-queues it, and then the stalled worker's publish call returns success.** Exactly one post, recorded `Published` with the stalled worker's receipt; the re-queued run finds nothing to claim or reconciles to the same receipt. Task 8 test `stalled_worker_late_success_is_recorded_once`.
2. **The account is reconnected long after a delivery parked for `account_needs_reconnect`.** Inside `park_grace` past its due time it resumes on the next sweep; outside it stays parked, `delivery_stale` is recorded once, and nothing publishes until the owner retries. Task 10 test `unpark_respects_park_grace_for_every_reason`.
3. **A ten-target job fails everywhere within a minute.** Ten in-app notifications and push messages, one email listing all ten. Task 17 test `ten_failures_send_one_email`.
4. **A delegate created a job, then the owner revoked the grant before the delivery failed.** The owner is notified; the former delegate gets nothing, and their old notifications still read correctly from their labels. Task 5 test `revoked_delegate_gets_nothing_new` and Task 19 test `old_notifications_keep_their_labels`.
5. **A browser blocks notifications, or the endpoint returns `410` after the user cleared site data.** The profile switch shows the blocked state and does not loop on permission prompts; the server deletes the gone endpoint on the first `410` and keeps sending to the user's other endpoints. Task 20 test `gone_endpoint_is_deleted_and_others_still_receive` and Task 21 test `blocked_permission_shows_blocked_state`.
6. **The server shuts down while a delivery is uploading.** The delivery stops at the next safe checkpoint, returns to `Pending` with `dispatch_gen + 1`, and publishes once after restart; a delivery past its final create call finishes within `server.shutdown_timeout`. Task 12 test `shutdown_at_checkpoint_requeues_and_publishes_once`.
7. **A user sets a time zone with no DST (`Asia/Riyadh`) or a 30-minute offset (`Asia/Kolkata`).** The digest goes out once per local day at 08:00 local time. Task 18 test `digest_runs_at_local_eight_for_offset_zones`.

---

### Task 1: Publishing queue and unknown job types re-queued (`postit-jobs`)

**Files:**
- Modify: `backend/crates/jobs/src/job.rs` (`Queue::Publishing`), `backend/crates/jobs/src/dispatch.rs`, `backend/crates/jobs/src/backend.rs` (apply the new outcome), `backend/crates/jobs/src/maintenance.rs` (`sweep_stranded` threshold), `backend/crates/data/src/job_outbox.rs` (`kill_unregistered` cutoff stays a parameter), `backend/crates/config/src/settings.rs` (`JobsSettings.unknown_type_max_age: Duration`, concurrency `publishing`, the three new schedules in `JobSchedules`), `backend/config/default.toml`, `docs/configuration.md`
- Test: `backend/crates/jobs/tests/unknown_types.rs` (create), `backend/crates/config` settings tests

**Interfaces:**
- Produces: `Queue::Publishing` (`as_str() == "publishing"`, `Queue::ALL` has 4 entries); `JobSchedules { delivery_due_sweep, account_preflight, notification_digest, .. }`; `JobsSettings.unknown_type_max_age` (default 7 days).
- Behavior: dispatching a job whose type the registry lacks no longer aborts. It re-queues the same job at `now + 60s` with no attempt consumed (attempt counter unchanged, no `job_attempts` failure row, ledger state `queued`, outbox row kept), logging the type at `info`. The maintenance sweep kills queued outbox rows of unregistered types only when they have been due for longer than `unknown_type_max_age` (it used the stranded threshold before).

- [ ] **Step 1: Write failing tests** in `tests/unknown_types.rs`: `unknown_type_is_requeued_without_consuming_an_attempt` (enqueue a `FutureJob` payload with `JOB_TYPE = "future_job"` through a queue, run one dispatch with a registry that lacks it; assert the ledger row is `queued`, `run_at == now + 60s`, attempt count 0, no failed attempt row); `unknown_type_runs_once_a_newer_worker_registers_it` (then dispatch with a registry that has it at `now + 61s`; handler runs once, row `succeeded`); `unregistered_type_killed_only_after_max_age` (due 6 days ago: kept; due 8 days ago: `killed` with `type_unregistered`). Settings test: `publishing_defaults` asserts the new defaults above.
- [ ] **Step 2: Run** `cargo test -p postit-jobs --test unknown_types` and `cargo test -p postit-config` — FAIL.
- [ ] **Step 3: Implement.** Add `Outcome::Requeue(DateTime<Utc>)` in `dispatch.rs`, returned for an unknown type; the backend applies it the way it applies a retry delay but without counting the attempt. Wire `[jobs.concurrency] publishing` into the worker's queue table.
- [ ] **Step 4: Run** the same — PASS. Run `cargo test -p postit-jobs` — PASS.
- [ ] **Step 5: Commit** `feat(jobs): add the publishing queue and re-queue unknown job types`.

### Task 2: Console disabled-action reasons as codes

**Files:**
- Modify: `backend/crates/jobs/src/console_spec.rs`, `backend/crates/jobs/src/console.rs` (drop `RETRIED_REASON`/`UNKNOWN_TYPE_REASON` strings), `backend/crates/identity/src/jobs.rs` (`delete_user` reason), every other `ConsoleSpec::disabled`/`restrict` caller, `backend/crates/api/src/dto_jobs.rs`, `web/src/features/jobs/JobActions.tsx`, `web/src/i18n/en-US/jobs.json`, `web/src/i18n/ar/jobs.json`
- Test: `backend/crates/jobs/src/console.rs` unit tests, `backend/crates/api/tests/jobs_console.rs` (or the existing console route test file), `web/src/features/jobs/JobActions.test.tsx`

**Interfaces:**
- Produces: `postit_jobs::DisabledReason { AlreadyRetried, TypeNotRegistered, MustFinish, PublishingManaged }` with `as_str()` (`already_retried`, `type_not_registered`, `must_finish`, `publishing_managed`) and serde snake_case. `ConsoleSpec::disabled(DisabledReason)`, `ConsoleSpec::restrict(Actions, DisabledReason)`, `ConsoleSpec::disabled_reason() -> Option<DisabledReason>`. API field `actions_disabled_reason: Option<DisabledReasonDto>` (OpenAPI enum of the four codes). Web `REASON_KEYS` keyed by code.

- [ ] **Step 1: Write failing tests**: jobs unit `allowed_actions_reports_codes` (unknown type → `TypeNotRegistered`; retried job → `AlreadyRetried`); API test asserting `"actions_disabled_reason": "must_finish"` for a `delete_user` job; web test rendering the translated text for each code in `en-US` and `ar`.
- [ ] **Step 2: Run** `cargo test -p postit-jobs`, the API test, and `pnpm test JobActions` (PowerShell, in `web/`) — FAIL.
- [ ] **Step 3: Implement**; add `publishing_managed` strings ("Publishing manages retries for this job" / Arabic). Run `cargo xtask openapi`.
- [ ] **Step 4: Run** the same plus `cargo xtask openapi --check` — PASS.
- [ ] **Step 5: Commit** `feat(jobs): report console disabled-action reasons as stable codes`.

### Task 3: Notification vocabulary (`postit-core`) and schema

**Files:**
- Create: `backend/crates/core/src/notification.rs`
- Modify: `backend/crates/core/src/lib.rs`, `backend/Cargo.toml` (`chrono-tz = "0"` in `[workspace.dependencies]`), `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/vault.rs` (`SEALED_COLUMNS` unchanged list check), `backend/crates/data/tests/schema.rs`
- Test: unit tests in `notification.rs`; `backend/crates/data/tests/schema.rs`

**Interfaces:**
- Produces in `postit_core`:
  - `Severity { Info, Critical }`.
  - `NotificationKind` (snake_case serde, `as_str`, `ALL`): `DeliveryDeadLettered`, `DeliveryFailed`, `DeliveryOutcomeUnknown`, `DeliveryParked`, `DeliveryStale`, `JobPartial`, `AccountNeedsReconnect`, `AccountRevokedByPlatform`, `ReconnectByPassed`, `RecurrenceSkipped` (all `Critical`); `DeliveryPublished`, `ReconnectReminder`, `DelegationChanged`, `NewComments`, `UserPending` (all `Info`). Methods `severity()`, `default_push() -> bool` (true for every `Critical` kind and `NewComments`), `default_email() -> bool` (true exactly for `Critical`), `email_allowed() -> bool` (`Critical` only).
  - `AccountLabel { account_id: AccountId, account_name: String, platform: PlatformId }`.
  - `DeliveryNotice { delivery_id: DeliveryId, job_id: PublishJobId, post_id: PostId, account: AccountLabel, park_reason: Option<ParkReason> }`, `JobNotice { job_id, post_id, published: u32, failed: u32 }`, `AccountNotice { account: AccountLabel }`, `ReminderStage { SevenDays, OneDay }`, `ReconnectNotice { grant_id: GrantId, accounts: Vec<AccountLabel>, reconnect_by: DateTime<Utc>, stage: Option<ReminderStage> }`, `SkipReason { AiProviderMissing, RewriteInvalid, JobActive }`, `RecurrenceNotice { recurrence_id, post_id, reason: SkipReason }`, `CommentsNotice { account: AccountLabel, count: u32 }`, `DelegationEvent { Granted, Changed, Revoked, Left, Expired, LimitEmptied }`, `DelegationNotice { delegation_id: DelegationId, event: DelegationEvent, party_name: String }`, `UserPendingNotice { user_id: UserId }`.
  - `Notice` (`#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]`, `openapi` feature derives `ToSchema`): one variant per kind carrying the struct above (`DeliveryDeadLettered(DeliveryNotice)` … `UserPending(UserPendingNotice)`), with `fn kind(&self) -> NotificationKind` and `fn payload_json(&self) -> serde_json::Value` (the inner struct only, as stored in `notifications.payload`) and `fn from_row(kind: &str, payload: Value) -> Result<Notice, serde_json::Error>`.
- Schema: `user_preferences` gains `notification_switches JSONB NOT NULL DEFAULT '{}'` (per-kind overrides `{ "<kind>": { "push": bool, "email": bool } }`), `comment_digest BOOLEAN NOT NULL DEFAULT false`, `last_comment_digest_at TIMESTAMPTZ`. `push_endpoints` gains `endpoint_hash BYTEA NOT NULL` with `UNIQUE (user_id, endpoint_hash)`, and `platform` CHECK allows `android`, `ios`, `web`. `notifications.kind` CHECK lists every `NotificationKind::as_str`; `severity` CHECK `info|critical`; index `notifications_unemailed_idx ON notifications (user_id) WHERE emailed_at IS NULL AND severity = 'critical'`.

- [ ] **Step 1: Write failing tests**: `notice_round_trips_with_kind_tag` (serialize `Notice::DeliveryFailed(..)`, expect `{"kind":"delivery_failed","payload":{...}}`, deserialize back); `from_row_reads_stored_payload`; `defaults_match_the_plan` (asserts the exact `default_push`/`default_email`/`severity` table above for all 15 kinds); `payload_has_no_free_text_fields_except_labels` (serialize one of each and assert the key set; guards against adding `text`); schema test `notification_kind_check_matches_core` (insert each `NotificationKind::as_str` succeeds; `"bogus"` fails) and `push_endpoint_hash_is_unique_per_user`.
- [ ] **Step 2: Run** `cargo test -p postit-core notification` and `cargo test -p postit-data --test schema` — FAIL.
- [ ] **Step 3: Implement**; reset the dev database.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(core): add the notification vocabulary and its schema`.

### Task 4: Notification, preference, and push-endpoint queries (`postit-data`)

**Files:**
- Create: `backend/crates/data/src/notifications.rs`, `backend/crates/data/src/push_endpoints.rs`
- Modify: `backend/crates/data/src/preferences.rs`, `backend/crates/data/src/delegations.rs`, `backend/crates/data/src/users.rs` (if no active-admin query exists there; reuse the one `identity::mail` uses if it does), `backend/crates/data/src/lib.rs`
- Test: `backend/crates/data/tests/notifications.rs`, `backend/crates/data/tests/push_endpoints.rs`, `backend/crates/data/tests/preferences.rs` (create or extend)

**Interfaces:**
- Produces:
  - `NewNotification { id: NotificationId, user: UserId, owner: Option<UserId>, notice: Notice, created_at: DateTime<Utc> }`; `NotificationRow { id, user, owner, notice: Notice, severity: Severity, read_at, emailed_at, pushed_at, created_at }`.
  - `NotificationRepo::insert(conn, &NewNotification) -> Result<(), DataError>`; `list(conn, user, Pagination) -> Result<ResultSet<NotificationRow>, DataError>` (newest first); `mark_read(conn, user, ReadSelection, now) -> Result<u64, DataError>` with `ReadSelection { Ids(Vec<NotificationId>), All }` (other users' IDs are ignored); `unread_state(conn, user) -> Result<UnreadState, DataError>` with `UnreadState { count: i64, newest: Option<NotificationId> }`; `has_unemailed_critical(conn, user) -> Result<bool, DataError>`; `take_unemailed_critical(conn, user, now) -> Result<Vec<NotificationRow>, DataError>` (`SELECT … FOR UPDATE`, then sets `emailed_at = now` on exactly those rows, oldest first); `comments_since(conn, user, since: Option<DateTime<Utc>>, until: DateTime<Utc>) -> Result<Vec<NotificationRow>, DataError>`; `get_for_user(conn, user, id) -> Result<NotificationRow, DataError>`; `mark_pushed(conn, id, at) -> Result<(), DataError>` (first success only).
  - `UserPreferences` gains `notification_switches: NotificationSwitches`, `comment_digest: bool`, `last_comment_digest_at: Option<DateTime<Utc>>`. `NotificationSwitches(BTreeMap<NotificationKind, ChannelSwitch>)` with `ChannelSwitch { push: Option<bool>, email: Option<bool> }` and `fn push(&self, kind) -> bool` / `fn email(&self, kind) -> bool` (override, else the kind default; `email` is always false when `!kind.email_allowed()`).
  - `PreferencesUpdate { locale: Option<String>, timezone: Option<String>, email_notifications: Option<bool>, switches: Option<NotificationSwitches>, comment_digest: Option<bool> }` and `UserPreferencesRepo::upsert(conn, user, &PreferencesUpdate) -> Result<UserPreferences, DataError>` (creates the row with the column defaults when missing, applies only `Some` fields; `switches` merges per kind). `UserPreferencesRepo::set_last_comment_digest_at(conn, user, at)`. `UserPreferencesRepo::digest_due(conn, now) -> Result<Vec<UserId>, DataError>` is **not** here (Task 18 computes local hours in Rust).
  - `NewPushEndpoint { id: PushEndpointId, user, endpoint: Sealed<String>, endpoint_hash: [u8; 32], p256dh: Sealed<String>, auth: Sealed<String>, platform: PushPlatform, device_label: String }`, `PushPlatform { Android, Ios, Web }`. `PushEndpointRepo::upsert(conn, &NewPushEndpoint) -> Result<PushEndpointId, DataError>` (same `(user, endpoint_hash)` replaces keys, label, platform; keeps the existing ID); `list_for_user(conn, user) -> Result<Vec<PushEndpointRow>, DataError>`; `delete(conn, user, id) -> Result<bool, DataError>`; `delete_by_id(conn, id)`; `get(conn, id) -> Result<Option<PushEndpointRow>, DataError>`; `mark_success(conn, id, at)`; `seal_context(id, column)`.
  - `DelegationRepo::live_grant_covers(conn, owner, delegate, account: Option<AccountId>) -> Result<bool, DataError>` (live = not revoked, not expired, delegate and owner active; `account` `Some` additionally requires an all-accounts grant or the account in its limit).
  - `UsersRepo::active_admin_ids(conn) -> Result<Vec<UserId>, DataError>` (reuse if it exists under another name).

- [ ] **Step 1: Write failing tests**: `mark_read_ignores_other_users_ids`, `unread_state_counts_and_newest`, `take_unemailed_critical_marks_exactly_what_it_returns` (an info row and an already-emailed row are untouched), `comments_since_bounds`, `switches_fall_back_to_kind_defaults`, `email_switch_never_on_for_info_kinds`, `upsert_creates_missing_row_and_applies_only_given_fields`, `upsert_merges_switches_per_kind`, `push_upsert_replaces_same_endpoint_keeps_id`, `push_endpoints_are_sealed` (raw column bytes do not contain the endpoint), `live_grant_covers_respects_limit_expiry_and_revocation`, plus the isolation checks (another user's notification and endpoint are not found).
- [ ] **Step 2: Run** `cargo test -p postit-data --test notifications --test push_endpoints --test preferences` — FAIL.
- [ ] **Step 3: Implement**; add the sealed columns' `SealContext`s (they already appear in `SEALED_COLUMNS`); `cargo sqlx prepare`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(data): add notification, preference, and push-endpoint queries`.

### Task 5: `postit-notify` crate and `Notifier::record`

**Files:**
- Create: `backend/crates/notify/Cargo.toml`, `backend/crates/notify/src/lib.rs`, `backend/crates/notify/src/record.rs`, `backend/crates/notify/src/error.rs`
- Modify: `backend/Cargo.toml` (member, `postit-notify` path dependency), `backend/crates/server/src/compose.rs` (build one `Notifier`)
- Test: `backend/crates/notify/tests/record.rs`

**Interfaces:**
- Produces:
  - `Recipients { Workspace { owner: UserId, actor: Option<UserId> }, User { user: UserId, owner: Option<UserId> }, Admins }`.
  - `NotifyEvent { recipients: Recipients, notice: Notice }`.
  - `NotifierDeps { ids: Arc<dyn IdGenerator>, clock: Arc<dyn Clock>, queue: JobQueue, outbox: MailOutbox, push_enabled: bool }`; `Notifier::new(NotifierDeps)`; `#[derive(Clone)]`.
  - `Notifier::record(&self, conn: &mut PgConnection, event: NotifyEvent) -> Result<Vec<NotificationId>, NotifyError>`: resolves recipients, inserts one row per recipient in `conn` (the caller's transaction). `Workspace`: the owner, plus `actor` when `actor != owner` and `DelegationRepo::live_grant_covers(owner, actor, account)` where `account` is the notice's `AccountLabel.account_id` when it has one. `User`: that user. `Admins`: every active admin, `owner_id` null. Deduplicates recipients. In this task it writes rows only; Tasks 17 and 20 add the email and push fan-out behind the same function.
  - `NotifyError { Data(DataError), Jobs(JobsError), Mail(MailError) }`, `impl From` each.
  - Test helper (feature `testkit`): `Notifier::for_tests(pool) -> Notifier` and `notifications_for(conn, user) -> Vec<NotificationRow>`.

- [ ] **Step 1: Write failing tests**: `owner_is_notified`, `live_delegate_actor_is_notified_too`, `revoked_delegate_gets_nothing_new` (Review Focus 4), `expired_delegate_gets_nothing`, `delegate_outside_limit_gets_nothing` (limited grant without the notice's account), `actor_equal_to_owner_gets_one_row`, `admins_get_deployment_notifications_with_null_owner`, `record_is_rolled_back_with_the_callers_transaction`.
- [ ] **Step 2: Run** `cargo test -p postit-notify` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(notify): record notifications for owners, live delegates, and admins`.

### Task 6: `postit-publishing` skeleton, bound clients, request assembly, and real assets in conformance

**Files:**
- Create: `backend/crates/publishing/Cargo.toml`, `src/lib.rs`, `src/deps.rs`, `src/jobs.rs` (payloads, `register`, `console`), `src/request.rs`
- Modify: `backend/Cargo.toml`, `backend/crates/oauth/src/service.rs` (public bound-client API), `backend/crates/oauth/src/lib.rs`, `backend/crates/agent-kit/src/testkit/conformance/mod.rs` and `checks.rs` (fixture assets), `backend/crates/agents/mock/tests/conformance.rs`, `backend/crates/server/src/compose.rs` (register publishing jobs and console specs on the worker role; build `PublishingService` for the API role)
- Test: `backend/crates/publishing/tests/request.rs`, `backend/crates/agent-kit/tests/conformance_http.rs`, `backend/crates/oauth/tests/bound_client.rs`

**Interfaces:**
- Consumes: Task 1 `Queue::Publishing`, Task 2 `DisabledReason::PublishingManaged`, Task 5 `Notifier`.
- Produces:
  - In `postit-oauth`: `BoundClient { client: Box<dyn AgentClient>, account: AccountRef, descriptor: AgentDescriptor, grant: GrantId, owner: UserId }` and `AccountService::bound_client(&self, owner: UserId, account: AccountId) -> Result<BoundClient, AccountError>` (system-side, no scope: the caller has already authorized; `AccountDisconnected` for a disconnected account, the catalog's `PlatformUnavailable`/`PlatformDisabled` errors unchanged). `AccountService::mark_needs_reconnect` stays the entry point for auth failures.
  - Job payloads (all `#[derive(Serialize, Deserialize)]`, UUID fields, `Queue::Publishing`): `PublishDelivery { delivery: Uuid, account: Uuid, dispatch_gen: i32 }` (`"publish_delivery"`), `PollDeliveryStatus { delivery: Uuid, account: Uuid, dispatch_gen: i32, poll: u32 }` (`"poll_delivery_status"`), `ReconcileDelivery { delivery: Uuid, account: Uuid, dispatch_gen: i32, attempt: u32 }` (`"reconcile_delivery"`), `DirectSendBatch { delivery: Uuid, dispatch_gen: i32 }` (`"direct_send_batch"`), `DeliveryDueSweep` (`"delivery_due_sweep"`, recurring, `Default`), `AccountPreflight` (`"account_preflight"`, recurring). Console specs: `id_field("delivery", ..)`, `id_field("account", ..)` where present, `disabled(DisabledReason::PublishingManaged)`; the two crons get `recurring(schedule)` with retry disabled the same way.
  - `PublishingDeps { pool, accounts: AccountService, media: MediaService, notifier: Notifier, queue: JobQueue, ids, clock, settings: PublishingSettings, shutdown: CancellationToken, compiled_platforms: Vec<PlatformId> }`; `PublishingService::new(PublishingDeps)` (`Clone`); `postit_publishing::register(&mut JobRegistry, PublishingService, &JobSchedules) -> Result<(), JobsError>`; `postit_publishing::console(&mut ConsoleCatalog, &JobSchedules)`.
  - `request::assemble(snapshot: &PublishSnapshot, account: AccountId, media: &MediaService, owner: UserId) -> Result<Assembled, AssembleError>` with `Assembled { variant: Variant, assets: Vec<MediaAsset> }` and `AssembleError { VariantMissing, MediaMissing(Vec<MediaId>), Data(DataError) }`. Assets resolve through `MediaService::assets(OwnerScope::own(owner), ids)`; an asset that no longer exists or is not `Ready` is `MediaMissing`.
  - Conformance: `ConformanceFixture.assets: Vec<MediaAsset>`, passed to `validate` and used by the `validate` check alongside the samples (closes the B5 deferred entry). The mock's conformance test builds real assets through `FsMediaStore` in a temp dir.
  - Job handlers in this task are stubs returning `Ok(())`; Tasks 7–12 fill them.

- [ ] **Step 1: Write failing tests**: `bound_client_for_disconnected_account_is_refused`, `bound_client_carries_descriptor_and_account_ref`; `assemble_resolves_assets_in_variant_order`, `assemble_reports_deleted_media`, `assemble_reports_missing_variant`; conformance `validate_check_receives_real_assets` (an agent whose `validate` records the asset count sees the fixture's assets); `publishing_jobs_are_registered_with_console_disabled` (registry has the six types on `publishing`; console spec reason `publishing_managed`).
- [ ] **Step 2: Run** `cargo test -p postit-publishing -p postit-oauth --test bound_client -p postit-agent-kit --features testkit` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS; `cargo check --workspace --all-targets --all-features`.
- [ ] **Step 5: Commit** `feat(publishing): add the publishing crate, bound clients, and request assembly with real media`.

### Task 7: Claim, pre-send checks, quota, revalidation, and parking

**Files:**
- Create: `backend/crates/publishing/src/worker.rs`, `src/checks.rs`, `src/quota.rs`, `src/notices.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`
- Test: `backend/crates/publishing/tests/checks.rs`, shared helpers in `backend/crates/publishing/tests/support/mod.rs`

**Interfaces:**
- Consumes: Task 6.
- Produces:
  - `worker::run_publish(service: &PublishingService, job: PublishDelivery) -> Result<(), JobError>`; in this task it claims (`DeliveryRepo::claim` with `settings.lease`; `None` → `Ok(())`), runs the checks, reserves quota, revalidates, and stops just before `StartRemote` (Task 8 continues from there).
  - `checks::park_reason(conn, &ClaimedDelivery, live_switch: bool, available: Availability) -> Result<Option<ParkReason>, DataError>`, in plan order: owner not `active` → `OwnerDisabled`; account not `active` → `AccountNeedsReconnect`; agent not compiled in → `PlatformUnavailable`; platform disabled → `PlatformDisabled`; live delivery with `app_settings.publishing.live` false → `LivePublishDisabled`. Dry-run deliveries skip the live check only.
  - `quota::reserve(conn, &ClaimedDelivery, &AgentDescriptor, op: QuotaOp, config: &Value, now) -> Result<Reservation, DataError>` with `QuotaOp { Publish, Comment }`, `Reservation { Reserved(Vec<ReservedUnit>), Exhausted }`, `quota::release(conn, &[ReservedUnit])`. One unit per descriptor bucket with a cost for the op; `period_start` from the bucket's `reset_time_zone` (via `chrono-tz`) and period; limit from `[platforms.<id>].quota.<bucket>` when the config sets it, else `default_limit`. Dry-run reserves nothing.
  - `notices::delivery_notice(conn, &ClaimedDelivery or DeliveryId) -> Result<DeliveryNotice, DataError>` (labels read from `connected_accounts`), used with `Notifier::record` and `Recipients::Workspace { owner, actor: job.created_by }`.
  - Parking: `DeliveryRepo::apply(Fence { expected: Claimed }, DeliveryEvent::Park(reason), ..)`; a live delivery records `Notice::DeliveryParked` in the same transaction. Quota exhausted parks `QuotaExceeded`. Revalidation (`validate_target` + the client's `validate` with the assembled assets, or `AssembleError`) failing → `RevalidationFailed` with the violations (codes and paths only) in `last_error`, plus `Notice::DeliveryFailed`.

- [ ] **Step 1: Write failing tests** (all with the `mock` agent through `bound_client`): `stale_dispatch_finds_nothing` (wrong `dispatch_gen`); `not_due_finds_nothing`; `disabled_owner_parks_owner_disabled`; `needs_reconnect_parks`; `uncompiled_platform_parks_platform_unavailable`; `disabled_platform_parks`; `kill_switch_parks_live_but_not_dry_run`; `parked_live_delivery_notifies_owner_once`; `exhausted_bucket_parks_quota_exceeded`; `app_bucket_shared_across_two_owners` (two owners, app bucket limit 1: second parks); `revalidation_failure_fails_with_codes_only`; `deleted_media_fails_revalidation`; `parking_never_degrades_to_dry_run` (live delivery parked, `delivery_attempts` empty, mock publish count 0).
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test checks` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): claim deliveries, park on failed checks, and reserve quota`.

### Task 8: Remote call, outcomes, retries, progress, and late receipts

**Files:**
- Create: `backend/crates/publishing/src/outcome.rs`, `src/progress.rs`, `src/backoff.rs`
- Modify: `backend/crates/publishing/src/worker.rs`
- Test: `backend/crates/publishing/tests/publish.rs`

**Interfaces:**
- Consumes: Task 7.
- Produces:
  - Worker continuation: `apply(StartRemote)` → `AttemptRepo::start` → spawn the lease renewer (`renew_lease` every `lease / 3`; a `false` renewal flips the progress's `lost` flag) → call `client.publish` (operation `publish`) or `client.commenter().comment` (operation `comment`; no `Commenter` → `PermanentFailure` with code `capability_unsupported`) with `PublishRequest { variant, mode: Live|DryRun, native_publish_at: job native schedule ? scheduled_at : None, container_ref, idempotency_key }` → `AttemptRepo::finish` → map the result.
  - `progress::DeliveryProgress` implements `PublishProgress`: `save_container_ref` writes `DeliveryRepo::set_container_ref` under the fence (a stale fence returns `AgentError::Cancelled`); `cancel_requested()` is true when the delivery's `cancel_requested_at` was seen set by the renewer or `shutdown` is cancelled (Task 12 uses the latter).
  - `outcome::map(result: Result<PublishOutcome, AgentError>, attempt_count: i32, max_attempts: u32, now, cancel: CancelCause) -> Mapped` with `Mapped { event: DeliveryEvent, data: TransitionData, follow_up: FollowUp, notice: Option<NotificationKind> }`, `FollowUp { None, Poll { at }, Reconcile, Redispatch, NeedsReconnect }`, `CancelCause { None, User, Shutdown }`:
    - `Published(r)` → `Published`; `Processing(r)` → `Accepted` + `Poll { at: now + 15s }` (or `remote_publish_at` when it is later, for native schedules); `Simulated` → `Simulated`.
    - `RetryClass::Retryable` / `RetryAfter(d)` → `RetryableFailure` with `next_attempt_at` from `backoff::retry_delay(attempt)` (or `d`), `count_attempt: true`; when `attempt_count + 1 >= max_attempts` → `RetryBudgetExhausted` (`DeadLetter`, notice `DeliveryDeadLettered`).
    - `Permanent` → `PermanentFailure` (notice `DeliveryFailed`); `NeedsReconnect` → `Park(AccountNeedsReconnect)` + `FollowUp::NeedsReconnect` (the worker calls `AccountService::mark_needs_reconnect(OwnerScope::own(owner), grant, <the system audit context the oauth jobs already use>)` after its commit); `QuotaExceeded` → `Park(QuotaExceeded)`; `Ambiguous` → `Ambiguous` + `Reconcile` (notice `DeliveryOutcomeUnknown`); `Cancelled` with `CancelCause::User` → `Cancel`, with `Shutdown` → `Shutdown` (`Pending`, generation + 1, redispatched).
    - `last_error` holds `{ "code": <AgentError code>, "class": <retry class> }`, never platform message text.
  - Release reserved quota when the result proves no remote call happened (`Cancelled` before the first mutating call, `Authentication` before send).
  - `Applied::Stale` with a `Published`/`Processing` receipt → `DeliveryRepo::record_late_receipt`; otherwise drop silently.
  - `backoff::retry_delay(attempt: i32) -> Duration` and `backoff::poll_delay(poll: u32) -> Duration` with the Global Constraints values; jitter from `rand`.
  - Notices on `DeliveryPublished` (info), `DeliveryFailed`, `DeliveryDeadLettered`, `DeliveryOutcomeUnknown`, and `JobPartial` when `refresh_job_status` returns `Partial` for a live job; `delivery_published`, `delivery_dead_lettered`, `delivery_outcome_unknown` audit events (IDs only).

- [ ] **Step 1: Write failing tests** with the mock's scripted `outcome` option: `published_records_receipt_and_notifies_info`; `processing_schedules_a_poll`; `retryable_failure_backs_off_with_new_generation`; `retry_budget_ends_in_dead_letter` (`max_attempts = 2`); `permanent_failure_fails`; `needs_reconnect_parks_and_marks_the_grant`; `rate_limited_uses_retry_after`; `outcome_unknown_enqueues_reconcile`; `dry_run_simulates_and_leaves_post_status` (`posts.status` unchanged, attempt recorded with the planned action); `comment_post_goes_through_commenter`; `container_ref_saved_before_next_step_and_reused_on_retry`; `stalled_worker_late_success_is_recorded_once` (Review Focus 1: renewer stopped, lease expired, sweep marks `OutcomeUnknown`, worker then returns `Published`: one `Published` row with that receipt, mock publish count 1); `duplicate_apalis_job_publishes_once` (two `PublishDelivery` with the same generation run concurrently: mock count 1); `concurrent_workers_publish_once` (10 tasks); `console_retry_of_every_publishing_type_publishes_nothing_twice` (re-enqueue each payload after success: mock count unchanged); `retry_after_edit_publishes_the_snapshot` (post edited after job creation: mock receives the snapshot text); `partial_job_notifies` (two targets, one permanent failure); `quota_released_when_no_call_was_made`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test publish` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): publish through agents with fenced outcomes, retries, and late receipts`.

### Task 9: Processing polls, timeouts, native schedules, and reconcile

**Files:**
- Create: `backend/crates/publishing/src/poll.rs`, `src/reconcile.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`, `backend/crates/agents/mock/src/client.rs` and `src/script.rs` (scripted reconcile answers)
- Test: `backend/crates/publishing/tests/processing.rs`, `backend/crates/agents/mock/tests/` (script test)

**Interfaces:**
- Consumes: Task 8.
- Produces:
  - Mock: post option `reconcile` with values `found` (default when the earlier publish landed), `absent`, `unresolvable` (returns `ReconcileOutcome::Unresolvable`), `unsupported` (returns `AgentError::Unsupported`), plus the same for comments through `reconcile_comment`.
  - `poll::run_poll(service, PollDeliveryStatus) -> Result<(), JobError>`: reads the delivery (`Processing`, matching generation, else exit); a native-scheduled delivery whose `remote_publish_at` is in the future and whose upload was accepted re-enqueues itself at `remote_publish_at`; otherwise `client.status(account, receipt)`: `Published` → `Published`; still `Processing` → re-enqueue with `poll + 1` at `now + poll_delay(poll + 1)`; `Permanent` → `PermanentFailure`; transport errors → re-enqueue. Past `processing_timeout` (from `remote_publish_at` for native schedules, else from the attempt start) → `ProcessingTimeout` (`OutcomeUnknown`) + reconcile. Direct deliveries are never polled here.
  - `reconcile::run_reconcile(service, ReconcileDelivery) -> Result<(), JobError>`: `OutcomeUnknown` with matching generation, else exit. Calls `reconcile` (or `reconcile_comment`) with the last attempt's start time and `container_ref`. `Found` → `ReconcileFound` with the receipt; `Absent` → `ReconcileAbsent` (redispatch, generation + 1); `Unresolvable` or `Unsupported` → `ReconcileUnresolvable` (`Parked(OutcomeUnresolvable)`, notice `DeliveryParked`); retryable errors re-enqueue with `attempt + 1` at `poll_delay(attempt + 1)` until `processing_timeout` since the attempt started, then `ReconcileUnresolvable`.

- [ ] **Step 1: Write failing tests**: `poll_publishes_when_platform_finishes`; `poll_backs_off_to_fifteen_minutes`; `processing_timeout_moves_to_outcome_unknown`; `native_schedule_timeout_counts_from_remote_publish_at`; `native_upload_waits_until_publish_time`; `reconcile_found_publishes`; `reconcile_absent_redispatches_once`; `reconcile_unresolvable_parks_and_notifies`; `reconcile_unsupported_parks` (spec gap: mock never returned it before); `comment_reconcile_uses_reconcile_comment`; `stale_reconcile_job_exits`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test processing` and the mock tests — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): poll processing deliveries and reconcile unknown outcomes`.

### Task 10: Due sweep and account preflight

**Files:**
- Create: `backend/crates/publishing/src/sweep.rs`, `src/preflight.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`, `backend/crates/data/src/publishing/sweep.rs` (only if a query is missing: deliveries due within an hour on accounts not checked in the last hour)
- Test: `backend/crates/publishing/tests/sweep.rs`, `backend/crates/publishing/tests/preflight.rs`

**Interfaces:**
- Consumes: Tasks 8 and 9.
- Produces:
  - `sweep::run_sweep(service) -> Result<SweepReport, JobError>` with `SweepReport { requeued: usize, reclaimed: usize, unknown: usize, orphaned_direct: usize, unparked: usize, stale: usize }`, in one pass with `limit = 500` per query: `overdue_pending(now, sweep_grace)` and `expired_claims` → enqueue `PublishDelivery` for each `Dispatch`; `expired_in_flight` → enqueue `ReconcileDelivery` (multi-step agents resume from `container_ref`); `orphaned_direct(now, sweep_grace)` → enqueue `DirectSendBatch`; `unpark(UnparkInput { now, park_grace, compiled_platforms, limit })` → enqueue `resumed`, and for each `needs_owner` record `Notice::DeliveryStale` (the B2b query already sets `parked_notified_at`; the worker never sets it at park time). Each enqueue runs in the transaction that produced the dispatch.
  - `preflight::run_preflight(service) -> Result<usize, JobError>`: accounts with a live delivery due within 1 hour and `last_checked_at` older than 1 hour (or null); `client.account_info`; active → `AccountRepo::mark_checked`; not active or `Authentication`/`InvalidCredentials` → `AccountService::mark_needs_reconnect` (Task 15 adds its notification); transport errors skip the account until the next run. No database connection is held across a platform call.

- [ ] **Step 1: Write failing tests**: `overdue_pending_is_requeued_with_new_generation`; `expired_claim_returns_to_pending_and_requeues`; `expired_in_flight_becomes_unknown_and_reconciles`; `orphaned_direct_batch_is_requeued_once_per_grace`; `unpark_respects_park_grace_for_every_reason` (Review Focus 2: for each of `owner_disabled`, `account_needs_reconnect`, `platform_unavailable`, `platform_disabled`, `live_publish_disabled`, `quota_exceeded`, cleared inside grace → resumed; outside → stays parked, one `delivery_stale` notice across two sweeps); `outcome_unresolvable_never_unparks`; `lost_queue_entry_delays_but_never_duplicates` (delete the apalis row; sweep republishes once); `preflight_detects_revoked_credential` (mock token `invalid` → grant `needs_reconnect`, delivery later parks); `preflight_skips_recently_checked_accounts`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test sweep --test preflight` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): add the due sweep and account preflight`.

### Task 11: Direct deliveries

**Files:**
- Create: `backend/crates/publishing/src/direct.rs`
- Modify: `backend/crates/publishing/src/worker.rs` (operation `direct`), `src/jobs.rs`
- Test: `backend/crates/publishing/tests/direct.rs`

**Interfaces:**
- Consumes: Tasks 7 and 8; `DirectRepo`, `UserLimitsRepo::get`, `postit_data::publishing::quota::EMAIL_CAP_BUCKET`.
- Produces:
  - Worker, operation `direct`: after the checks, `StartRemote`, then the audience from the variant's options (`lists: [ContactListId]`, `tags: [String]`, as `Audience`). Dry-run: count with the materialize query's count form (add `DirectRepo::audience_count(conn, owner, &Audience)` if absent), record it in the attempt, end `Simulated`, write nothing to `direct_sends`. Live: `DirectRepo::materialize`, `apply(Accepted)` (`Processing`, `direct_counts.audience` set), enqueue `DirectSendBatch { delivery, dispatch_gen }`, all in one transaction.
  - `direct::run_batch(service, DirectSendBatch) -> Result<(), JobError>`: `claim_batch(Fence { expected: Processing }, size = settings.email.batch_size, lease, cap)` with `cap = Some((&AccountQuotaKey { bucket: EMAIL_CAP_BUCKET, period_start: owner's local date }, user_limits.email_daily_cap))` (0 = unlimited → `None`). `Rows` → load recipient fields (`contacts` email, first and last name, custom fields, locale) for those rows, build `DirectRecipient`s, call `client.direct().send_batch(account, recipients, variant)`, renew the batch lease during the call; map each result: `Sent` → `DirectResult::Sent`, `Bounced { permanent: true }` → `Bounced` + a suppression through the existing `add_hash` in `data/src/contacts.rs` with the bounced reason, `Bounced { permanent: false }` and `Failed { retryable: true }` → leave queued for the next batch until the row's third try, then `Failed`; `Failed { retryable: false }` → `Failed`. In the same transaction as the last `record`: `finish_if_done` (all terminal → `Published` with counts, notice `DeliveryPublished`); otherwise enqueue the next `DirectSendBatch`. `CapReached` → `resume_at` = the next local midnight in the owner's time zone, `wait_for_cap`, enqueue at `resume_at`. `Empty` → `finish_if_done`. `Stale` → exit.
  - A whole-batch `AgentError` (transport) leaves the rows leased; their lease expiry returns them to `queued` and the sweep's `orphaned_direct` re-enqueues.

- [ ] **Step 1: Write failing tests** with `mock_direct` and contacts inserted directly: `audience_materialized_once_and_fixed` (adding a contact after materialization does not add a row); `suppressed_and_unsubscribed_are_skipped`; `batches_send_every_recipient_once`; `cap_boundary_waits_for_next_local_day` (cap 3, audience 5: 3 sent, delivery `Processing`, next batch at local midnight, then 2 more); `bounce_5xx_suppresses`; `soft_failure_retries_within_budget`; `crash_mid_batch_resumes_without_resending_terminal_rows`; `direct_dry_run_reports_count_and_writes_nothing`; `campaign_with_some_failures_is_published_with_counts`; `processing_timeout_exempts_direct_with_queued_rows`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test direct` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): send direct deliveries in capped batches`.

### Task 12: Service API, cancellation checkpoints, and graceful shutdown

**Files:**
- Create: `backend/crates/publishing/src/service.rs`, `src/error.rs`
- Modify: `backend/crates/publishing/src/worker.rs`, `src/progress.rs`, `backend/crates/server/src/lib.rs` (pass the shutdown token into `PublishingDeps`), `backend/crates/data/src/audit.rs` (publishing audit kinds), `web/src/i18n/*/audit.json` (labels)
- Test: `backend/crates/publishing/tests/service.rs`, `backend/crates/publishing/tests/shutdown.rs`

**Interfaces:**
- Consumes: Tasks 6–11; `PostRepo::lock`, `JobRepo`, `DeliveryRepo`, `validate_target`.
- Produces on `PublishingService` (all take `scope: &OwnerScope` and `audit: AuditContext`; all errors are `PublishingError`):
  - `create_job(scope, post: PostId, input: PublishInput, audit) -> Result<CreatedJob, PublishingError>` with `PublishInput { targets: Vec<AccountId>, at: Option<DateTime<Utc>>, native_schedule: bool, dry_run: bool, confirm: bool, if_match: PostVersion }`. Locks the post, checks `if_match` (`VersionConflict`), requires `Capability::Edit` for dry-run and `Capability::Publish` for live plus every target in limit, `confirm` for live (`ConfirmationRequired`), treats `at` up to 1 minute in the past as now (earlier → violation `schedule_in_past`), validates every target (`ValidationFailed(Vec<Violation>)`), builds the snapshot with `resolve_variant`, `JobRepo::create`, enqueues each `Dispatch` with `enqueue_in`, records `publish_confirmed` (live), all in one transaction. Native schedule applies only to targets whose agent declares it; the rest use postit scheduling.
  - `reschedule(scope, job, at, if_match: i64, audit) -> Result<RescheduleOutcome, PublishingError>`; `RescheduleOutcome { moved: Vec<DeliveryId>, failed: Vec<DeliveryId> }`. `JobRepo::reschedule_pending`, enqueue its dispatches, then each `needs_agent` delivery through `NativeScheduler::reschedule` one at a time (a failure is listed; moved ones keep their time). `NotReschedulable` when the repo refuses. Audit `publish_job_rescheduled`.
  - `cancel_job(scope, job, audit) -> Result<CancelReport, PublishingError>` and `cancel_delivery(scope, delivery, audit) -> Result<CancelReport, PublishingError>`; `CancelReport { cancelled: Vec<DeliveryId>, requested: Vec<DeliveryId>, not_cancellable: Vec<DeliveryId> }`. `NeedsAgent` → `NativeScheduler::delete` then cancel; no scheduler or a failed delete → `not_cancellable`. Audit `publish_job_cancelled` / `delivery_cancelled`.
  - `retry(scope, delivery, confirm: bool, acknowledge_outcome_unknown: bool, audit) -> Result<Dispatch, PublishingError>`: `confirm` required, revalidates the snapshot variant with assembled assets (deleted media → `ValidationFailed`), `DeliveryRepo::retry` (maps `AcknowledgementRequired`, `JobActive`), enqueue. Audit `delivery_retried`.
  - `resolve(scope, delivery, outcome: ResolvedOutcome, audit) -> Result<DeliveryState, PublishingError>`. Audit `delivery_resolved`.
  - `PublishingError` variants map one-to-one to the plan's codes: `Scope`, `NotFound`, `VersionConflict`, `ConfirmationRequired`, `AcknowledgementRequired`, `ValidationFailed(Vec<Violation>)`, `NotReschedulable`, `JobActive`, `PostLocked`, `Data`, `Jobs`. (`postit-api` mapping comes with D3.)
  - Cancellation at checkpoints: the renewer polls `cancel_requested_at` each renewal and sets the flag `DeliveryProgress::cancel_requested` reads; a cancel arriving after the final create call is ignored by the agent and the delivery finishes.
  - Graceful shutdown: `PublishingDeps.shutdown` cancelled → `cancel_requested()` true; an agent stopping at a checkpoint returns `Cancelled`, mapped with `CancelCause::Shutdown` (Task 8). Publishing handlers do not start a new claim after shutdown begins (they return `Ok(())` and the sweep picks the delivery up after restart).
  - Audit kinds `PublishConfirmed`, `PublishJobRescheduled`, `PublishJobCancelled`, `DeliveryCancelled`, `DeliveryRetried`, `DeliveryResolved`, `DeliveryPublished`, `DeliveryDeadLettered`, `DeliveryOutcomeUnknown` (add any missing; web labels in English and Arabic).

- [ ] **Step 1: Write failing tests**: `stale_if_match_is_refused`; `live_publish_needs_confirm`; `schedule_up_to_a_minute_past_is_now`; `mixed_native_and_postit_targets_in_one_job`; `edit_delegate_can_dry_run_but_not_publish_live`; `limited_delegate_cannot_target_outside_limit`; `reschedule_moves_pending_and_native_uploads`; `reschedule_refused_while_in_flight`; `cancel_at_each_safe_checkpoint` (before first mutating call; between chunks with the mock's multi-step script); `cancel_after_final_call_finishes`; `native_cancel_with_and_without_agent_delete`; `outcome_unknown_cannot_be_cancelled`; `retry_requires_confirm_and_acknowledgement`; `retry_refused_while_post_has_another_live_job` (`job_active`); `resolve_from_dead_letter`; `jobs_survive_delegation_revocation` (delegate creates, grant revoked, delivery still publishes); `shutdown_at_checkpoint_requeues_and_publishes_once` (Review Focus 6); `shutdown_after_final_call_finishes_within_grace`; isolation matrix for every service call (owner, `view`/`edit`/`publish` delegates, limited delegate, unrelated user).
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test service --test shutdown` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): add the publishing service, cancellation checkpoints, and graceful shutdown`.

### Task 13: Deletion steps and the disconnect hook

**Files:**
- Create: `backend/crates/publishing/src/deletion.rs`, `src/disconnect.rs`, `backend/crates/media/src/deletion.rs`
- Modify: `backend/crates/oauth/src/disconnect.rs`, `backend/crates/oauth/src/service.rs` (`AccountServiceDeps.disconnect_hook: Option<Arc<dyn DisconnectHook>>`), `backend/crates/oauth/src/lib.rs`, `backend/crates/data/src/media.rs` (list a user's storage keys), `backend/crates/server/src/compose.rs` (step order and hook injection)
- Test: `backend/crates/publishing/tests/deletion.rs`, `backend/crates/publishing/tests/disconnect.rs`, `backend/crates/media/tests/deletion.rs`

**Interfaces:**
- Produces:
  - In `postit-oauth`: `#[async_trait] pub trait DisconnectHook: Send + Sync { async fn before_disconnect(&self, owner: UserId, account: AccountId) -> Result<NativeCleanup, AccountError>; }` with `NativeCleanup { deleted: u32, remaining: u32 }`. `disconnect` calls it before its transaction (credentials still valid), then proceeds; `DisconnectOutcome.native_scheduled_remaining` = `remaining`. Without a hook (tests, API role without publishing) behavior is today's.
  - `postit_publishing::PublishingDisconnect` implements it: deletes each native-scheduled upload of the account through `NativeScheduler::delete` and cancels it; uploads the agent cannot delete stay `remaining`. It also cancels queued `direct_sends` rows of a `Processing` direct delivery on that account (`DirectRepo::cancel_queued`), then `finish_if_done` (spec: disconnect stops a running campaign).
  - `postit_publishing::PublishingDeletionStep` (`name() == "publishing"`): cancels the user's pending and parked deliveries, requests cancel on in-flight ones, deletes native-scheduled uploads (as the hook does), and returns `JobError::Retry` while any of the user's deliveries is `claimed` or `in_flight`. Idempotent.
  - `postit_media::MediaDeletionStep` (`name() == "media"`): lists the user's storage keys (`MediaRepo::storage_keys_for_owner`), removes each file through the `MediaStore` (missing file = done). Closes the B4 deferred entry.
  - Server registers steps in order: publishing, oauth (exists), media.

- [ ] **Step 1: Write failing tests**: `disconnect_deletes_native_uploads_through_the_agent`; `disconnect_lists_uploads_the_agent_cannot_delete`; `disconnect_stops_a_running_campaign` (queued rows `failed`/cancelled, sent rows stay sent); `disconnect_keeps_history`; `delete_user_with_pending_in_flight_and_native_deliveries` (retries until in-flight finishes, then completes); `publishing_step_is_idempotent`; `media_step_removes_files_and_tolerates_missing`; `steps_run_in_order` (recording fakes in the identity handler test).
- [ ] **Step 2: Run** the three test files — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): add the publishing and media deletion steps and the disconnect hook`.

### Task 14: Account and reconnect notifications (`postit-oauth`)

**Files:**
- Modify: `backend/crates/oauth/Cargo.toml` (`postit-notify`), `backend/crates/oauth/src/service.rs` (`AccountServiceDeps.notifier: Notifier`), `src/manage.rs` (`mark_needs_reconnect`), `src/events.rs`, `src/jobs.rs` (`run_refresh_credentials` stages), `src/refresh.rs`, `backend/crates/data/src/accounts.rs` (`GrantRepo::reconnect_warnings_due(now) -> Vec<(GrantId, UserId, ReminderStage)>`), `backend/crates/oauth/src/testkit.rs`
- Test: `backend/crates/oauth/tests/notifications.rs`

**Interfaces:**
- Consumes: Task 5.
- Produces:
  - `mark_needs_reconnect` records one `Notice::AccountNeedsReconnect` per affected account (labels), `Recipients::Workspace { owner, actor: None }`, in its transaction. `refresh_grant`'s `invalid_grant` path goes through the same function (or records the same notices in its transaction).
  - Platform deauthorization records `Notice::AccountRevokedByPlatform` per account.
  - `refresh_credentials` stages: for active grants with `reconnect_by`: within 7 days and `reconnect_warned = 'none'` → `Notice::ReconnectReminder { stage: SevenDays }`, set `7d`; within 1 day and warned `none|7d` → `OneDay`, set `1d`; passed → existing `needs_reconnect` + `Notice::ReconnectByPassed`, set `expired`. Each stage once; a grant first seen inside 1 day gets only the `OneDay` reminder.

- [ ] **Step 1: Write failing tests**: `needs_reconnect_notifies_owner_per_account`; `invalid_grant_on_refresh_notifies`; `deauthorize_notifies_revoked`; `reminders_at_seven_days_and_one_day_once_each`; `grant_first_seen_inside_one_day_gets_one_reminder`; `reconnect_by_passed_notifies_critically`; `delegates_are_not_told_about_reconnects` (no actor on these events).
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test notifications` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS, and `cargo test -p postit-oauth`.
- [ ] **Step 5: Commit** `feat(oauth): notify owners about reconnects, revocations, and reconnect deadlines`.

### Task 15: Delegation and pending-user notifications (`postit-identity`)

**Files:**
- Modify: `backend/crates/identity/Cargo.toml` (`postit-notify`), `backend/crates/identity/src/delegation.rs` (grant, change, revoke, leave, expiry), `src/claims.rs` (pending sign-in), `backend/crates/oauth/src/disconnect.rs` and `backend/crates/data/src/accounts.rs` (`AccountRepo::disconnect` returns delegations whose limit became empty: `Vec<(DelegationId, UserId /* delegate */)>`), `backend/crates/server/src/compose.rs`
- Test: `backend/crates/identity/tests/delegation_notifications.rs`, `backend/crates/oauth/tests/disconnect.rs` (extend)

**Interfaces:**
- Consumes: Task 5.
- Produces: `Notice::DelegationChanged(DelegationNotice)` to the other party (`Recipients::User { user, owner: Some(owner) }`, `party_name` = the acting party's display name): grant, change, revoke → delegate; leave → owner; expiry → both (each with the other's name). A limited grant emptied by a disconnect → `LimitEmptied` to both parties. A sign-in that leaves a user `pending` records `Notice::UserPending { user_id }` with `Recipients::Admins`, alongside the existing approval emails (no second email). The B6 deferred entry closes.

- [ ] **Step 1: Write failing tests**: `grant_change_revoke_notify_the_delegate`; `leave_notifies_the_owner`; `expiry_notifies_both`; `emptied_limit_notifies_both`; `pending_sign_in_notifies_every_active_admin_once`; `notification_payload_names_only_the_party` (no email address in the payload).
- [ ] **Step 2: Run** `cargo test -p postit-identity --test delegation_notifications` and the oauth disconnect test — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(identity): notify the other party of delegation changes and admins of pending users`.

### Task 16: Preferences: partial update, time zone, switches, locale enum

**Files:**
- Modify: `backend/crates/api/src/routes/me.rs`, `backend/crates/api/src/dto.rs`, `backend/crates/api/Cargo.toml` (`chrono-tz`)
- Test: `backend/crates/api/tests/preferences.rs` (create or extend)

**Interfaces:**
- Consumes: Task 4.
- Produces:
  - `PreferencesDto { locale: LocaleDto, timezone: String, email_notifications: bool, comment_digest: bool, switches: Vec<KindSwitchDto> }` with `KindSwitchDto { kind: NotificationKind, push: bool, email: Option<bool> }` (one entry per kind in `NotificationKind::ALL` order; `email` is `None` for kinds where `!email_allowed()`).
  - `UpdatePreferences` (`deny_unknown_fields`, every field optional, `null` = absent): `locale: Option<LocaleDto>` (OpenAPI enum `en-US`, `ar`), `timezone: Option<String>` (IANA name parsed with `chrono_tz::Tz`; invalid → 422 `validation_failed` with detail naming the field), `email_notifications: Option<bool>`, `comment_digest: Option<bool>`, `switches: Option<Vec<KindSwitchUpdate>>` with `KindSwitchUpdate { kind: NotificationKind, push: Option<bool>, email: Option<bool> }` (`email` on an info kind → 422).
  - `PUT` creates a missing row (no more 404). `GET` and `PUT` keep answering `owner_only` under act-as and stay reachable while pending.

- [ ] **Step 1: Write failing tests**: `put_creates_missing_row`; `put_applies_only_given_fields`; `null_fields_are_unchanged` (Dart client behavior); `invalid_timezone_is_422`; `email_switch_on_info_kind_is_422`; `get_returns_every_kind_with_defaults`; `locale_is_an_enum_in_openapi` (assert the schema in the generated document); `act_as_is_owner_only`.
- [ ] **Step 2: Run** `cargo test -p postit-api --test preferences` — FAIL.
- [ ] **Step 3: Implement**; `cargo xtask openapi`; add the new strings to the mobile and web error maps only if new codes appear (none expected here).
- [ ] **Step 4: Run** the tests and `cargo xtask openapi --check` — PASS; `flutter analyze && flutter test` in `mobile/` (PowerShell) since the Dart client changed.
- [ ] **Step 5: Commit** `feat(api): make preferences a partial update with time zone and notification switches`.

### Task 17: Coalesced critical email

**Files:**
- Create: `backend/crates/notify/src/email.rs`, `backend/crates/mail/templates/notification_alert.html`, `notification_alert.txt`, `backend/crates/i18n/locales/en-US/notify.ftl`, `backend/crates/i18n/locales/ar/notify.ftl`
- Modify: `backend/crates/mail/src/outbox.rs` (`MailKind::NotificationAlert`, `MailKind::CommentDigest`, `NAMES`), `backend/crates/mail/src/templates.rs`, `backend/crates/i18n/src/catalog.rs` (load `notify.ftl`), `backend/crates/notify/src/record.rs` (email fan-out), `backend/crates/notify/src/lib.rs` (`register_loaders`), `backend/crates/server/src/compose.rs`
- Test: `backend/crates/notify/tests/email.rs`, snapshot files beside it

**Interfaces:**
- Consumes: Tasks 4, 5.
- Produces:
  - `record` fan-out: for each recipient row whose kind is critical, the recipient's `email_notifications` is on, and `NotificationSwitches::email(kind)` is on, enqueue `send_email { kind: NotificationAlert, recipient, params: MailParams::None }` at `now + ALERT_DELAY`, but only when `has_unemailed_critical` was false before this insert (an earlier alert job will pick it up).
  - `NotificationAlertLoader` implements `MailContextLoader`: `load` calls `take_unemailed_critical(recipient, now)` (marking happens here, inside the handler's transaction, which rolls back if SMTP fails); empty → `Skip`; otherwise renders one email listing every notice (one line each from its kind and labels, in the recipient's locale) with a link to `{app.public_url}/`. `mark_sent` is a no-op for this kind. *(The loader marks in `load` because `mark_sent` does not receive which rows were included; the transaction makes it equivalent.)*
  - Subject `{ $count -> [one] Something needs your attention on postit *[other] { $count } things need your attention on postit }`; Arabic equivalents in `ar/notify.ftl`. Lines per kind keyed `notice-<kind>` with `$account`, `$platform`, `$count`, `$reason` arguments. Templates on the shared `layout.html`.

- [ ] **Step 1: Write failing tests**: `ten_failures_send_one_email` (Review Focus 3: ten `DeliveryFailed` notices for one user in one minute → one `send_email` job, its run renders ten lines, all ten rows `emailed_at` set); `email_switch_off_sends_nothing`; `master_switch_off_sends_nothing`; `info_kinds_never_email`; `smtp_failure_leaves_rows_unemailed` (handler transaction rolled back); `later_job_skips_when_nothing_left`; snapshots `alert_en`, `alert_ar` (`dir="rtl"`); no address, token, or post text in the rendered email beyond the recipient's own address header (leak harness).
- [ ] **Step 2: Run** `cargo test -p postit-notify --test email` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(notify): email critical notifications in one coalesced message`.

### Task 18: Daily comment digest

**Files:**
- Create: `backend/crates/notify/src/digest.rs`, `backend/crates/mail/templates/comment_digest.html`, `comment_digest.txt`
- Modify: `backend/crates/notify/src/lib.rs` (`register`, `console`), `backend/crates/i18n/locales/*/notify.ftl`, `backend/crates/server/src/compose.rs`
- Test: `backend/crates/notify/tests/digest.rs`

**Interfaces:**
- Produces:
  - `NotificationDigest` recurring job (`"notification_digest"`, `Queue::Maintenance`, schedule `notification_digest`, `RetryPolicy::Backoff` like the other crons): for every user with `comment_digest = true` and `email_notifications = true`, computes the local time in `user_preferences.timezone` (`chrono_tz`); when the local hour is `DIGEST_HOUR` and `last_comment_digest_at` is before today's local midnight (or null), enqueues `send_email { kind: CommentDigest, recipient, params: None }`.
  - `CommentDigestLoader`: `load` reads `comments_since(user, last_comment_digest_at, now)`; empty → `Skip`; otherwise one line per account with the summed count. `mark_sent` sets `last_comment_digest_at = at`.
  - Tests drive it with `Notice::NewComments` rows inserted through `Notifier::record` (no producer until E2).

- [ ] **Step 1: Write failing tests**: `digest_runs_at_local_eight_for_offset_zones` (Review Focus 7: `Asia/Riyadh`, `Asia/Kolkata`, `America/New_York` across a DST change: exactly one enqueue per local day, at local 08:00); `digest_off_sends_nothing`; `no_comments_skips`; `counts_are_summed_per_account`; snapshots `digest_en`, `digest_ar`.
- [ ] **Step 2: Run** `cargo test -p postit-notify --test digest` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(notify): send the daily comment digest at 8 a.m. local time`.

### Task 19: Notification routes

**Files:**
- Create: `backend/crates/api/src/routes/notifications.rs`
- Modify: `backend/crates/api/src/routes/mod.rs`, `backend/crates/api/src/dto.rs` (or a new `dto_notifications.rs` beside `dto_jobs.rs`), `backend/crates/api/src/openapi.rs`, `backend/crates/core/Cargo.toml` (`openapi` feature derives for `Notice`)
- Test: `backend/crates/api/tests/notifications.rs`

**Interfaces:**
- Consumes: Task 4.
- Produces:
  - `GET /api/v1/notifications?page&per_page` → `ResultSet<NotificationDto>`; `NotificationDto { id, kind, severity, owner_id: Option<Uuid>, payload, read_at, created_at }` with `kind`/`payload` from `Notice` (OpenAPI `oneOf` discriminated by `kind`).
  - `POST /api/v1/notifications/read` with `{ ids: [Uuid] }` or `{ all: true }` → `{ updated: u64 }`.
  - `GET /api/v1/notifications/unread-count` → `{ count: i64 }` with `ETag: W/"<count>-<newest id or 0>"`; `If-None-Match` equal → 304 with no body.
  - All self only (`ActiveUser`), 403 `owner_only` under act-as. A delegate's list includes rows from workspaces they act in (`owner_id` tells which).

- [ ] **Step 1: Write failing tests**: `list_is_newest_first_and_paginated`; `read_by_ids_ignores_other_users`; `read_all`; `unread_count_etag_and_304`; `act_as_is_owner_only`; `delegate_sees_workspace_notifications_with_owner_id`; `old_notifications_keep_their_labels` (Review Focus 4: rename the account and revoke the grant; the delegate's old row still carries the original labels); `pending_user_cannot_list` only if `ActiveUser` refuses pending (assert the current extractor's behavior).
- [ ] **Step 2: Run** `cargo test -p postit-api --test notifications` — FAIL.
- [ ] **Step 3: Implement**; `cargo xtask openapi`.
- [ ] **Step 4: Run** tests and `cargo xtask openapi --check` — PASS; mobile `flutter analyze && flutter test`.
- [ ] **Step 5: Commit** `feat(api): add the notification list, read, and unread-count routes`.

### Task 20: Web Push sending and endpoint routes

**Files:**
- Create: `backend/crates/notify/src/push.rs`, `backend/crates/notify/src/vapid.rs`, `backend/crates/api/src/routes/push.rs`, `backend/crates/xtask/src/vapid.rs`
- Modify: `backend/Cargo.toml` (the Web Push crate), `backend/crates/notify/Cargo.toml`, `backend/crates/notify/src/record.rs` (push fan-out), `backend/crates/api/src/routes/mod.rs`, `backend/crates/api/src/error.rs` (`push_disabled`, `push_endpoint_invalid`), `backend/crates/config/src/settings.rs` (startup check: `push.enabled` requires key and subject in every environment), `backend/config/development.toml` (`[push] enabled = true`, `vapid_subject = "mailto:admin@postit.local"`), `!ref/vault/development/postit.env` (extracted file only: `POSTIT__PUSH__VAPID_PRIVATE_KEY=`), `backend/crates/xtask/src/main.rs` (`vapid-keygen`), `backend/crates/server/src/compose.rs` (public push client), the stack doctor check (`docker/` launcher's doctor script or `server` doctor module, wherever the identity-mail key check lives), `web/src/api/errors*` and `mobile` error maps
- Test: `backend/crates/notify/tests/push.rs`, `backend/crates/api/tests/push.rs`

**Interfaces:**
- Consumes: Tasks 4, 5.
- Produces:
  - **Phase-start check:** pick the Web Push crate. It must build the encrypted body (RFC 8291 `aes128gcm`) and the VAPID `Authorization` header (RFC 8292) without sending (expected: `web-push` with `default-features = false`, using its request builder), license in `deny.toml`, maintained, no Arabic-derived name. Record the choice in the commit message. If none fits, implement with `ece` + `p256` + `jsonwebtoken` (all workspace or MPL/MIT) and keep the RFC 8291 vectors.
  - `vapid::VapidKeys::from_settings(&PushSettings) -> Result<Option<VapidKeys>, NotifyError>` (`None` when disabled); `VapidKeys::public_key_b64url(&self) -> String` (uncompressed P-256, base64url without padding). `cargo xtask vapid-keygen` prints `POSTIT__PUSH__VAPID_PRIVATE_KEY=<b64url>` and the public key.
  - `PushSend { notification: Uuid, endpoint: Uuid }` job (`"push_send"`, `Queue::Default`, `RetryPolicy::Backoff { max_attempts: Some(5), initial: 30s, max: 30min }`, console id fields `notification`, `endpoint`). Handler: load endpoint (gone → done) and notification (gone → done), render title and body in the recipient's locale from `notify.ftl` (`push-title-<kind>`, `push-body-<kind>`), payload JSON `{ "id", "kind", "title", "body" }`, encrypt, send through the public client with `TTL: 86400` and `Urgency: high` for critical kinds (`normal` otherwise). `2xx` → `mark_success`, `mark_pushed`; `404`/`410` → `delete_by_id`, done; `429`/`5xx`/transport → `JobError::Retry`; other `4xx` → done with a warning (no endpoint in the log).
  - `record` fan-out: when `push_enabled` and `NotificationSwitches::push(kind)`, enqueue one `PushSend` per endpoint of the recipient.
  - Routes (self, `owner_only` under act-as): `GET /api/v1/push/vapid-public-key` → `{ key }` or 404 `push_disabled`; `POST /api/v1/me/push-endpoints` with `{ endpoint, p256dh, auth, platform: android|ios|web, device_label }` → 201 `{ id }` (endpoint must be `https`, `p256dh` a 65-byte uncompressed point and `auth` 16 bytes, both base64url; otherwise 422 `push_endpoint_invalid`; 404 `push_disabled` when off); `DELETE /api/v1/me/push-endpoints/{id}` → 204 (another user's ID → 404). Endpoint hash = SHA-256 of the endpoint URL (`emixcrypto`).
  - The address policy applies at send time (a private endpoint fails as permanent unless `allow_private_endpoints`).

- [ ] **Step 1: Write failing tests**: `rfc8291_test_vector` (encrypt with the RFC's fixed keys and salt; compare the body); `vapid_header_shape` (`vapid t=<jwt>, k=<key>`, JWT `aud` = endpoint origin, `exp` ≤ 24h, `sub` = subject); `push_delivers_payload_without_text` (wiremock endpoint; decrypted payload has only id, kind, title, body; no post text); `gone_endpoint_is_deleted_and_others_still_receive` (Review Focus 5: two endpoints, one `410`); `rate_limited_endpoint_retries`; `private_endpoint_refused_unless_allowed`; `push_switch_off_enqueues_nothing`; `push_disabled_enqueues_nothing`; routes: `vapid_key_404_when_disabled`, `register_validates_keys`, `re_register_replaces`, `delete_other_users_endpoint_is_404`, `act_as_is_owner_only`; leak harness: no endpoint URL or key in logs, errors, or problem bodies.
- [ ] **Step 2: Run** `cargo test -p postit-notify --test push` and `cargo test -p postit-api --test push` — FAIL.
- [ ] **Step 3: Implement**; generate the dev key pair with `cargo xtask vapid-keygen` into the extracted dev vault (the maintainer re-packs `vault.7z`); `cargo xtask openapi`.
- [ ] **Step 4: Run** the tests, `cargo xtask openapi --check`, mobile analyze and test — PASS.
- [ ] **Step 5: Commit** `feat(notify): send Web Push with VAPID and manage push endpoints`.

### Task 21: Browser push and notification settings (web)

**Files:**
- Create: `web/public/sw.js`, `web/src/features/me/NotificationSettings.tsx`, `web/src/features/me/NotificationSettings.test.tsx`, `web/src/features/me/useBrowserPush.ts`, `web/src/features/me/usePreferences.ts`, `web/src/features/me/sw.test.ts`
- Modify: `web/src/features/me/ProfilePage.tsx`, `web/src/main.tsx` (register the service worker when supported), `web/src/i18n/en-US/me.json`, `web/src/i18n/ar/me.json`, `backend/crates/server/src/web.rs` (CSP `worker-src 'self'`; `sw.js` served with `Cache-Control: no-cache`), `backend/crates/server/tests/web.rs`
- Test: the web tests above, `backend/crates/server/tests/web.rs`

**Interfaces:**
- Consumes: Tasks 16, 19, 20 (generated `schema.d.ts`).
- Produces:
  - `sw.js` (plain JS, root scope): `push` → `showNotification(title, { body, tag: id, data: { id } })`; `notificationclick` → close, focus an open postit client or open one, navigate to `/?notification=<id>`.
  - `useBrowserPush()` → `{ state: 'unsupported' | 'blocked' | 'off' | 'on' | 'working', enable(), disable() }`. `enable` asks permission once (no prompt when `Notification.permission === 'denied'`), subscribes with `applicationServerKey` from `GET /push/vapid-public-key`, posts `{ endpoint, p256dh, auth, platform: 'web', device_label }` (label `"<browser> on <os>"` from the user agent, fallback `"Browser"`), stores the returned ID per subscription in `localStorage` (try/catch). `disable` unsubscribes and deletes the endpoint. A 404 `push_disabled` shows `unsupported` with a "not enabled on this server" note.
  - `NotificationSettings` on the profile page, built from the D0b kit (`FormField`, `OptionSelect`, switches from `components/ui`): the browser push switch, the master email switch, the digest switch, the time-zone select (IANA list from `Intl.supportedValuesOf('timeZone')`, prefilled with the browser's zone when the stored value is the default `UTC`, saved on change), and a per-kind table of push and email switches (email column only where the API returns `email`). Saves with partial `PUT /me/preferences`.
  - Strings in English and Arabic for every kind name and state; right-to-left layout checked.

- [ ] **Step 1: Write failing tests**: `blocked_permission_shows_blocked_state` (Review Focus 5: mocked `Notification.permission = 'denied'`; no `requestPermission` call); `enable_subscribes_and_registers`; `disable_unsubscribes_and_deletes`; `push_disabled_server_shows_unsupported`; `timezone_prefilled_from_browser_when_default`; `kind_switch_sends_partial_update`; `renders_rtl_in_arabic`; `sw.test.ts`: `push_event_shows_notification`, `click_focuses_and_navigates`; server `csp_allows_same_origin_workers`.
- [ ] **Step 2: Run** `pnpm test` in `web/` (PowerShell) and `cargo test -p postit-server --test web` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the web gates from `web/CLAUDE.md` and the server test — PASS. Manually: `cargo dev` + `pnpm dev`, enable browser notifications on the profile page, trigger a mock failure through a test-only route or `psql` insert plus `Notifier` call, see the browser notification (record what was done in the commit body).
- [ ] **Step 5: Commit** `feat(web): add browser push and notification settings to the profile page`.

### Task 22: Docs, deferred files, and full gates

**Files:**
- Modify: `docs/configuration.md` (`[push]`, the new `[jobs]` keys and schedules, `[publishing]` notes), `docs/deployment.md` (VAPID key generation for qa and production in the G1 checklist; push stays off until then), `docs/security.md` (push payload rule, endpoint sealing, labels in notification payloads), `!ref/plans/01. vision and architecture.md` (`notifications` payload: "IDs, counts, and display labels"), `CLAUDE.md` (project state: D1 built), `backend/crates/data/src/publishing/mod.rs` (job map), `docs/superpowers/plans/2026-10-24-b5-agent-kit-mock-deferred.md` → `git mv` to `…-deferred-done.md` with a resolution note, `docs/superpowers/plans/2026-10-28-b7-oauth-connection-deferred.md` → `…-deferred-done.md`, `docs/superpowers/plans/2026-10-26-b4-media-deferred.md` and `2026-10-26-b6-delegation-deferred.md` (mark the resolved entries; keep names), `docs/superpowers/plans/2026-10-29-d1-publishing-engine-notifications-deferred.md` (only if something truly cannot be fixed in D1)

- [ ] **Step 1: Run**, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3`, `cargo sqlx prepare --check --workspace`, `cargo xtask openapi --check`, `cargo deny check` (the production audit job's command). From `web/`: the `web/CLAUDE.md` gates. From `mobile/`: the format, analyze, and test commands in `CLAUDE.md`. Expected: all pass; fix anything that does not, minor issues included.
- [ ] **Step 2: Update the docs and deferred files** listed above. Each resolved deferred entry names the D1 task that resolved it.
- [ ] **Step 3: Exit-criteria check** against plan 03 D1: queued work durable, safely claimed, observable, retryable, idempotent (Tasks 7–12 tests); no duplicate after a remote success (Review Focus 1); notifications reach in-app, email, and push as configured (Tasks 17–21). Write the result in the final report.
- [ ] **Step 4: Commit** `docs: record D1 in the configuration, deployment, and security docs and close its deferred items`.
