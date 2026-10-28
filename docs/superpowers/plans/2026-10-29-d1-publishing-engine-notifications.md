# D1: Publishing Engine, Notifications, and Push Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make queued publishing real (durable, safely claimed, observable, retryable, idempotent, never duplicated after a remote success) on the mock agents, and let postit tell people things: in-app notifications, coalesced critical emails, a daily comment digest, and Web Push to Android (UnifiedPush) and browsers.

**Architecture:**
- `postit-jobs` gains the `publishing` queue, re-queues unknown job types instead of killing them, and reports console disabled-action reasons as codes.
- `postit-core` gains the notification vocabulary (`NotificationKind`, `Severity`, `Notice` and its payload structs). `postit-data` gains the notification, push-endpoint, and preference queries; the delivery queries already exist (B2b, `data/src/publishing/`).
- New `postit-notify` (plan 01 level 6) records notifications inside the caller's transaction (`Notifier::record`), resolves recipients, and fans out to email (`MailContextLoader`s for a coalesced alert and the digest) and push (`push_send`, RFC 8291 + VAPID through `postit-http`).
- New `postit-publishing` (level 10) owns the publishing jobs, the worker flow, the due sweep, preflight, the direct flow, the service API D3 will wrap, the publishing and media deletion steps, and the disconnect hook `postit-oauth` calls.
- `postit-oauth` and `postit-identity` become notification producers (reconnect, revocation, `reconnect_by` stages, delegation changes, pending users). `postit-api` adds the notification, push, and widened preference routes. The web client gets a service worker, a CSP update, and the notification settings on the profile page.

**Tech Stack:** Rust 2024, axum 0.8, sqlx (Postgres), apalis behind `postit-jobs`, askama + Fluent (`postit-i18n`) for email and push text, `postit-http` for every outbound request, wiremock, utoipa; React 19 + Vite + Vitest in `web/`. New workspace dependencies: `chrono-tz` (MIT/Apache-2.0) and the Web Push implementation chosen in Task 20: a crate only if it builds without OpenSSL (`web-push` pulls `ece`, whose default backend is OpenSSL, so check that first), otherwise RustCrypto `aes-gcm` (MIT/Apache-2.0) beside the workspace's `p256`, `hkdf`, and `jsonwebtoken`.

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
- Every park must leave the delivery in a state `Sweep::unpark` still sees as blocked. Otherwise the next sweep resumes it, it parks again, and each round records a new critical notice. `quota_exceeded` sets `TransitionData.next_attempt_at` to the bucket's next period start (or the agent's `retry_after`). `platform_unavailable` is used only when the agent is not compiled in, which is the predicate `unpark` checks.
- `Sweep::*` functions open and commit their own transaction. To enqueue in the transaction that produced a dispatch, the caller opens a transaction, passes it in (the inner `begin` becomes a savepoint), enqueues, then commits.
- Settings (exist, `[publishing]`): `live = true`, `max_attempts = 5`, `lease = "5min"` (renew every `lease / 3`), `sweep_grace = "2min"`, `park_grace = "15min"`, `processing_timeout = "24h"`. New: `[jobs] unknown_type_max_age = "7days"`, `[jobs.concurrency] publishing = 4`, `[jobs.schedules] delivery_due_sweep = "0 * * * * *"`, `account_preflight = "0 */10 * * * *"`, `notification_digest = "0 0 * * * *"`. `[push]` (exists): `enabled`, `vapid_private_key` (secret), `vapid_subject`, `allow_private_endpoints`.
- Backoff constants (spec leaves the values to the plan): retry `min(30min, 30s × 2^attempt)` with full jitter (uniform in `[0, that]`); processing polls `min(15min, 15s × 2^poll)`, no jitter; a `RetryAfter(d)` from an agent wins over the computed delay.
- Critical-email delay: `ALERT_DELAY = 2min`. Digest hour: `DIGEST_HOUR = 8` (local, `user_preferences.timezone`).
- Notification payloads carry IDs, counts, and the labels the spec names (account display name, platform ID, the other party's display name for delegation kinds). Never post or comment text, addresses, tokens, endpoint URLs, or keys, in payloads, emails, push bodies, audit details, logs, or job payloads. Endpoint URLs in logs only as `Sensitivity::Opaque`.
- Dry-run deliveries and jobs record no notifications (no `delivery_parked`, `delivery_published`, `delivery_failed`, `job_partial`, or the others); their attempt rows are the record.
- Every new owned-data or self route gets the four-caller isolation tests (owner, delegate at each level, delegate limited to other accounts, unrelated user); the notification, push, and preference routes answer 403 `owner_only` under `X-Postit-Act-As`.
- New problem codes: `push_disabled` (404 on `GET /push/vapid-public-key` when push is off), `push_endpoint_invalid` (422). Both get web and mobile error-map entries in English and Arabic.
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=D:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from the crate that gained them. Web tasks also run the gates in `web/CLAUDE.md` through PowerShell with pnpm (never npm). A task that changes the API contract runs `cargo xtask openapi` and commits `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`.
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`. American English everywhere, including Arabic-catalog keys' English comments.

## Review Focus

1. **A worker stalls past its lease mid-publish, the sweep marks the delivery `OutcomeUnknown`, and then the stalled worker's publish call returns success.** Two orders:
   - (a) The late success arrives while the delivery is still `OutcomeUnknown`. `record_late_receipt` publishes it with that receipt, and the reconcile job exits.
   - (b) Reconcile ran first, found nothing, and re-queued the delivery. The new run and the late call reach the platform with the same `idempotency_key`, and the platform returns the one post to both. The new run records it, and the stalled worker's write is `Stale` and dropped.

   Exactly one post either way. (b) relies on the platform deduplicating by idempotency key, as the mock does. For a platform that does not, the agent's reconcile is the only guard, and the conformance `idempotency` check records which case applies. Task 8 tests `stalled_worker_late_success_is_recorded_once` and `late_success_after_reconcile_absent_publishes_once`.
2. **The account is reconnected long after a delivery parked for `account_needs_reconnect`.** Inside `park_grace` past its due time it resumes on the next sweep; outside it stays parked, `delivery_stale` is recorded once, and nothing publishes until the owner retries. Task 10 test `unpark_respects_park_grace_for_every_reason`.
3. **A ten-target job fails everywhere within a minute.** Ten in-app notifications. Ten push messages that share one `tag` and `Topic` (the job ID), so the push service and the device keep only the latest. One email listing all ten. Task 17 test `ten_failures_send_one_email`, Task 20 test `job_pushes_share_a_topic`.
4. **A delegate created a job, then the owner revoked the grant before the delivery failed.** The owner is notified; the former delegate gets nothing, and their old notifications still read correctly from their labels. Task 5 test `revoked_delegate_gets_nothing_new` and Task 19 test `old_notifications_keep_their_labels`.
5. **A browser blocks notifications, or the endpoint returns `410` after the user cleared site data.** The profile switch shows the blocked state and does not loop on permission prompts; the server deletes the gone endpoint on the first `410` and keeps sending to the user's other endpoints. Task 20 test `gone_endpoint_is_deleted_and_others_still_receive` and Task 21 test `blocked_permission_shows_blocked_state`.
6. **The server shuts down while a delivery is uploading.** The delivery stops at the next safe checkpoint, returns to `Pending` with `dispatch_gen + 1`, and publishes once after restart; a delivery past its final create call finishes within `server.shutdown_timeout`. Task 12 test `shutdown_at_checkpoint_requeues_and_publishes_once`.
7. **A user sets a time zone with no DST (`Asia/Riyadh`) or a 30-minute offset (`Asia/Kolkata`).** The digest goes out once per local day, in the cron run that falls in local hour 8. The cron runs at minute 0 UTC, so that run is at 08:00 in Riyadh and 08:30 in Kolkata. Task 18 test `digest_runs_at_local_eight_for_offset_zones`.
8. **A user turns off email for one critical kind, then gets a notification of that kind and later one of another critical kind. Or a notification is recorded while an alert email job is running.** The opted-out row never appears in an email and never blocks later ones. The row recorded during the run is emailed by its own job. Task 17 tests `opted_out_kind_never_blocks_later_alerts` and `row_recorded_during_a_send_is_emailed_next`.
9. **A delivery parks because a quota bucket is used up.** It stays parked until the bucket's next period (or the agent's `retry_after`), records one `delivery_parked` notice, and resumes once. Task 7 test `quota_park_waits_for_the_period_and_notifies_once`, Task 10 test `parked_delivery_stays_parked_while_its_cause_persists`.

---

### Task 1: Publishing queue and unknown job types re-queued (`postit-jobs`)

**Files:**
- Modify: `backend/crates/jobs/src/job.rs` (`Queue::Publishing`), `backend/crates/jobs/src/dispatch.rs`, `backend/crates/jobs/src/backend.rs` (apply the new outcome), `backend/crates/jobs/src/apalis_sql.rs` (`requeue_unknown`), `backend/crates/jobs/src/ledger.rs` (no attempt recorded for a requeue), `backend/crates/jobs/src/maintenance.rs` (`kill_unregistered` cutoff), `backend/crates/data/src/job_outbox.rs` (`kill_unregistered` cutoff stays a parameter), `backend/crates/config/src/settings.rs` (`JobsSettings.unknown_type_max_age: Duration`, concurrency `publishing`, the three new schedules in `JobSchedules`), `backend/config/default.toml`, `docs/configuration.md`
- Test: `backend/crates/jobs/tests/unknown_types.rs` (create), `backend/crates/config` settings tests

**Interfaces:**
- Produces: `Queue::Publishing` (`as_str() == "publishing"`, `Queue::ALL` has 4 entries); `JobSchedules { delivery_due_sweep, account_preflight, notification_digest, .. }`; `JobsSettings.unknown_type_max_age` (default 7 days).
- Behavior:
  - **When dispatch meets an unknown type.** Relay moves only registered types (`relay.rs`, `JobOutboxRepo::claim_batch`). So dispatch meets an unknown type only when a newer worker relayed the job and an older worker fetched it. The job then lives in apalis storage and has no outbox row.
  - **Re-queue within `unknown_type_max_age`.** While the job's ledger `created_at` is within `unknown_type_max_age`, dispatch returns `Outcome::Requeue(Duration::from_secs(60))`. The backend then runs one statement, `UPDATE apalis.jobs SET run_at = now() + 60s, max_attempts = max_attempts + 1`, in a new `apalis_sql::requeue_unknown` beside `defer_next_run`. It then returns the plain error. The ack sets `status = 'Failed'` and `attempts = n` (`APALIS_NOTES.md`, plain `Err`), so the row stays re-fetchable (`attempts < max_attempts`) without spending the type's budget. Lowering `attempts` instead would not work, because the ack overwrites it.
  - **Ledger.** The ledger records no attempt and no `job_attempts` row, keeps the job `queued` with `run_at = now + 60s`, and logs the type at `info`.
  - **Past `unknown_type_max_age`.** Dispatch aborts as it does today (`ErrorKind::UnknownType`, ledger `killed`), so a type removed for good stops re-queuing.
  - **Attempt number.** The `JobContext.attempt` of a later run must not count requeues. Where apalis's `Attempt::current()` does count them, derive the context attempt from the ledger's attempt count instead.
  - **Outbox backstop.** Separately, the maintenance sweep's `kill_unregistered` (outbox rows no worker ever relayed) uses `unknown_type_max_age` as its cutoff instead of the stranded threshold.

- [ ] **Step 1: Write failing tests** in `tests/unknown_types.rs`:
  - `unknown_type_is_requeued_without_consuming_an_attempt`: relay a `FutureJob` (`JOB_TYPE = "future_job"`, `RetryPolicy::None`) through a registry that has it, then dispatch with a registry that lacks it. Assert the ledger row is `queued` with `run_at == now + 60s` and no `job_attempts` row, and the apalis row is re-fetchable.
  - `unknown_type_runs_once_a_newer_worker_registers_it`: then dispatch with a registry that has it at `now + 61s`. The handler runs once with `JobContext.attempt == 1`, and the row ends `succeeded`.
  - `requeue_keeps_the_retry_budget`: a `Backoff { max_attempts: Some(3) }` type re-queued twice still gets three tries.
  - `unknown_type_aborts_after_max_age`: with ledger `created_at` 8 days ago, dispatch kills it with `ErrorKind::UnknownType`.
  - `unregistered_outbox_row_killed_only_after_max_age`: an outbox row due 6 days ago is kept; one due 8 days ago is `killed` with `type_unregistered`.

  Settings test: `publishing_defaults` asserts the new defaults above.
- [ ] **Step 2: Run** `cargo test -p postit-jobs --test unknown_types` and `cargo test -p postit-config` — FAIL.
- [ ] **Step 3: Implement.** Add `Outcome::Requeue(Duration)` in `dispatch.rs`, returned for an unknown type within the age limit. Wire `[jobs.concurrency] publishing` into the worker's queue table.
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
- Schema: `user_preferences` gains `notification_switches JSONB NOT NULL DEFAULT '{}'` (per-kind overrides `{ "<kind>": { "push": bool, "email": bool } }`), `comment_digest BOOLEAN NOT NULL DEFAULT false`, `last_comment_digest_at TIMESTAMPTZ`. `push_endpoints` gains `endpoint_hash BYTEA NOT NULL` with `UNIQUE (user_id, endpoint_hash)`, and `platform` CHECK allows `android`, `ios`, `web`. `notifications.kind` CHECK lists every `NotificationKind::as_str`; `severity` CHECK `info|critical`; index `notifications_unemailed_idx ON notifications (user_id) WHERE emailed_at IS NULL AND severity = 'critical'`; index `notifications_unread_idx ON notifications (user_id) WHERE read_at IS NULL` (the unread-count route is polled often). `emailed_at` means "settled for email": set when the row was emailed (alert or digest), or at insert when no email is wanted.

- [ ] **Step 1: Write failing tests**: `notice_round_trips_with_kind_tag` (serialize `Notice::DeliveryFailed(..)`, expect `{"kind":"delivery_failed","payload":{...}}`, deserialize back); `from_row_reads_stored_payload`; `defaults_match_the_plan` (asserts the exact `default_push`/`default_email`/`severity` table above for all 15 kinds); `payload_has_no_free_text_fields_except_labels` (serialize one of each and assert the key set; guards against adding `text`); schema test `notification_kind_check_matches_core` (insert each `NotificationKind::as_str` succeeds; `"bogus"` fails) and `push_endpoint_hash_is_unique_per_user`.
- [ ] **Step 2: Run** `cargo test -p postit-core notification` and `cargo test -p postit-data --test schema` — FAIL.
- [ ] **Step 3: Implement**; reset the dev database.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(core): add the notification vocabulary and its schema`.

### Task 4: Notification, preference, and push-endpoint queries (`postit-data`)

**Files:**
- Create: `backend/crates/data/src/notifications.rs`, `backend/crates/data/src/push_endpoints.rs`
- Modify: `backend/crates/data/src/preferences.rs`, `backend/crates/data/src/delegations.rs`, `backend/crates/data/src/lib.rs`
- Test: `backend/crates/data/tests/notifications.rs`, `backend/crates/data/tests/push_endpoints.rs`, `backend/crates/data/tests/preferences.rs` (create or extend)

**Interfaces:**
- Produces:
  - `NewNotification { id: NotificationId, user: UserId, owner: Option<UserId>, notice: Notice, email_wanted: bool, created_at: DateTime<Utc> }`. `email_wanted: false` inserts the row with `emailed_at = created_at`, so it never waits for, appears in, or blocks an email. `NotificationRow { id, user, owner, notice: Notice, severity: Severity, read_at, emailed_at, pushed_at, created_at }`.
  - `NotificationRepo::insert(conn, &NewNotification) -> Result<(), DataError>`; `list(conn, user, Pagination) -> Result<ResultSet<NotificationRow>, DataError>` (newest first); `mark_read(conn, user, ReadSelection, now) -> Result<u64, DataError>` with `ReadSelection { Ids(Vec<NotificationId>), All }` (other users' IDs are ignored); `unread_state(conn, user) -> Result<UnreadState, DataError>` with `UnreadState { count: i64, newest: Option<NotificationId> }`; `take_unemailed_critical(conn, user, now) -> Result<Vec<NotificationRow>, DataError>` (`SELECT … FOR UPDATE`, then sets `emailed_at = now` on exactly those rows, oldest first); `take_undigested_comments(conn, user, now) -> Result<Vec<NotificationRow>, DataError>` (the same for `new_comments` rows with `emailed_at IS NULL`; marking rows instead of comparing times means a row whose transaction commits late is never skipped); `get_for_user(conn, user, id) -> Result<NotificationRow, DataError>`; `mark_pushed(conn, id, at) -> Result<(), DataError>` (first success only).
  - `UserPreferences` gains `notification_switches: NotificationSwitches`, `comment_digest: bool`, `last_comment_digest_at: Option<DateTime<Utc>>`. `NotificationSwitches(BTreeMap<NotificationKind, ChannelSwitch>)` with `ChannelSwitch { push: Option<bool>, email: Option<bool> }` and `fn push(&self, kind) -> bool` / `fn email(&self, kind) -> bool` (override, else the kind default; `email` is always false when `!kind.email_allowed()`).
  - `PreferencesUpdate { locale: Option<String>, timezone: Option<String>, email_notifications: Option<bool>, switches: Option<NotificationSwitches>, comment_digest: Option<bool> }` and `UserPreferencesRepo::upsert(conn, user, &PreferencesUpdate) -> Result<UserPreferences, DataError>` (creates the row with the column defaults when missing, applies only `Some` fields; `switches` merges per kind). `UserPreferencesRepo::set_last_comment_digest_at(conn, user, at)`. `UserPreferencesRepo::digest_due(conn, now) -> Result<Vec<UserId>, DataError>` is **not** here (Task 18 computes local hours in Rust).
  - `NewPushEndpoint { id: PushEndpointId, user, endpoint: Sealed<String>, endpoint_hash: [u8; 32], p256dh: Sealed<String>, auth: Sealed<String>, platform: PushPlatform, device_label: String }`, `PushPlatform { Android, Ios, Web }`. `PushEndpointRepo::upsert(conn, &NewPushEndpoint) -> Result<PushEndpointId, DataError>` (same `(user, endpoint_hash)` replaces keys, label, platform; keeps the existing ID); `list_for_user(conn, user) -> Result<Vec<PushEndpointRow>, DataError>`; `delete(conn, user, id) -> Result<bool, DataError>`; `delete_by_id(conn, id)`; `get(conn, id) -> Result<Option<PushEndpointRow>, DataError>`; `mark_success(conn, id, at)`; `seal_context(id, column)`.
  - `DelegationRepo::live_grant_covers(conn, owner, delegate, account: Option<AccountId>) -> Result<bool, DataError>` (live = not revoked, not expired, delegate and owner active; `account` `Some` additionally requires an all-accounts grant or the account in its limit).
  - Admin recipients reuse the existing `UsersRepo::list_active_admin_ids` (`backend/crates/data/src/users.rs:597`).

- [ ] **Step 1: Write failing tests**: `mark_read_ignores_other_users_ids`, `unread_state_counts_and_newest`, `take_unemailed_critical_marks_exactly_what_it_returns` (an info row and an already-emailed row are untouched), `row_not_wanted_by_email_is_settled_at_insert`, `take_undigested_comments_marks_what_it_returns`, `switches_fall_back_to_kind_defaults`, `email_switch_never_on_for_info_kinds`, `upsert_creates_missing_row_and_applies_only_given_fields`, `upsert_merges_switches_per_kind`, `push_upsert_replaces_same_endpoint_keeps_id`, `push_endpoints_are_sealed` (raw column bytes do not contain the endpoint), `live_grant_covers_respects_limit_expiry_and_revocation`, plus the isolation checks (another user's notification and endpoint are not found).
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
  - `Notifier::record(&self, conn: &mut PgConnection, event: NotifyEvent) -> Result<Vec<NotificationId>, NotifyError>`: resolves recipients, inserts one row per recipient in `conn` (the caller's transaction). `Workspace`: the owner, plus `actor` when `actor != owner` and `DelegationRepo::live_grant_covers(owner, actor, account)` where `account` is the notice's `AccountLabel.account_id` when it has one. `User`: that user. `Admins`: every active admin, `owner_id` null. Deduplicates recipients. In this task it writes rows only, all with `email_wanted: false`. Task 17 computes `email_wanted` and adds the email fan-out, and Task 20 adds push, behind the same function.
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
- Modify: `backend/Cargo.toml`, `backend/crates/oauth/src/service.rs` (public bound-client API), `backend/crates/oauth/src/lib.rs`, `backend/crates/data/src/audit.rs` (publishing audit kinds), `web/src/i18n/en-US/audit.json`, `web/src/i18n/ar/audit.json` (their labels), `backend/crates/agent-kit/src/testkit/conformance/mod.rs` and `checks.rs` (fixture assets), `backend/crates/agents/mock/tests/conformance.rs`, `backend/crates/server/src/compose.rs` (register publishing jobs and console specs on the worker role; build `PublishingService` for the API role)
- Test: `backend/crates/publishing/tests/request.rs`, `backend/crates/agent-kit/tests/conformance_http.rs`, `backend/crates/oauth/tests/bound_client.rs`

**Interfaces:**
- Consumes: Task 1 `Queue::Publishing`, Task 2 `DisabledReason::PublishingManaged`, Task 5 `Notifier`.
- Produces:
  - In `postit-oauth`: `BoundClient { client: Box<dyn AgentClient>, account: AccountRef, descriptor: AgentDescriptor, grant: GrantId, owner: UserId }` and `AccountService::bound_client(&self, owner: UserId, account: AccountId) -> Result<BoundClient, AccountError>` (system-side, no scope: the caller has already authorized; `AccountDisconnected` for a disconnected account, the catalog's `PlatformUnavailable`/`PlatformDisabled` errors unchanged). `AccountService::mark_needs_reconnect` stays the entry point for auth failures.
  - Job payloads (all `#[derive(Serialize, Deserialize)]`, UUID fields, `Queue::Publishing`), each buildable from a B2b `Dispatch { delivery, dispatch_gen, run_at }`, which carries no account (handlers read the account from the claim or the row):
    - `PublishDelivery { delivery: Uuid, dispatch_gen: i32 }` (`"publish_delivery"`)
    - `PollDeliveryStatus { delivery: Uuid, dispatch_gen: i32, poll: i32 }` (`"poll_delivery_status"`)
    - `ReconcileDelivery { delivery: Uuid, dispatch_gen: i32, attempt: i32 }` (`"reconcile_delivery"`)
    - `DirectSendBatch { delivery: Uuid, dispatch_gen: i32 }` (`"direct_send_batch"`)
    - `DeliveryDueSweep` (`"delivery_due_sweep"`, recurring, `Default`)
    - `AccountPreflight` (`"account_preflight"`, recurring)

    Console specs: `id_field("delivery", ..)` and `disabled(DisabledReason::PublishingManaged)`; the two crons get `recurring(schedule)`, with retry disabled the same way.
  - `AuditEventKind` gains `PublishConfirmed`, `PublishJobRescheduled`, `PublishJobCancelled`, `DeliveryCancelled`, `DeliveryRetried`, `DeliveryResolved`, `DeliveryPublished`, `DeliveryDeadLettered`, and `DeliveryOutcomeUnknown` (snake_case names, IDs and counts only in details), with web labels in English and Arabic. They land here because Task 8 already writes three of them.
  - `PublishingDeps { pool, accounts: AccountService, media: MediaService, notifier: Notifier, queue: JobQueue, ids, clock, settings: PublishingSettings, shutdown: CancellationToken, compiled_platforms: Vec<PlatformId> }`; `PublishingService::new(PublishingDeps)` (`Clone`); `postit_publishing::register(&mut JobRegistry, PublishingService, &JobSchedules) -> Result<(), JobsError>`; `postit_publishing::console(&mut ConsoleCatalog, &JobSchedules)`.
  - `request::assemble(snapshot: &PublishSnapshot, account: AccountId, media: &MediaService, owner: UserId) -> Result<Assembled, AssembleError>` with `Assembled { variant: Variant, assets: Vec<MediaAsset> }` and `AssembleError { VariantMissing, MediaMissing(Vec<MediaId>), Data(DataError) }`. Assets resolve through `MediaService::assets(OwnerScope::own(owner), ids)`; an asset that no longer exists or is not `Ready` is `MediaMissing`.
  - Conformance: `ConformanceFixture.assets: Vec<MediaAsset>`, passed to `validate` and used by the `validate` check alongside the samples (closes the B5 deferred entry). The mock's conformance test builds real assets through `FsMediaStore` in a temp dir.
  - Job handlers in this task are stubs returning `Ok(())`; Tasks 7–12 fill them.

- [ ] **Step 1: Write failing tests**: `bound_client_for_disconnected_account_is_refused`, `bound_client_carries_descriptor_and_account_ref`; `assemble_resolves_assets_in_variant_order`, `assemble_reports_deleted_media`, `assemble_reports_missing_variant`; conformance `validate_check_receives_real_assets` (an agent whose `validate` records the asset count sees the fixture's assets); `publishing_jobs_are_registered_with_console_disabled` (registry has the six types on `publishing`; console spec reason `publishing_managed`); `publishing_audit_kinds_round_trip` (every new kind's `as_str` parses back through `FromStr`); the web catalogs test (`web/src/i18n/catalogs.test.ts`) keeps `en-US` and `ar` `audit.json` in step.
- [ ] **Step 2: Run** `cargo test -p postit-publishing -p postit-oauth --test bound_client -p postit-agent-kit --features testkit` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS; `cargo check --workspace --all-targets --all-features`.
- [ ] **Step 5: Commit** `feat(publishing): add the publishing crate, bound clients, and request assembly with real media`.

### Task 7: Claim, pre-send checks, quota, revalidation, and parking

**Files:**
- Create: `backend/crates/publishing/src/worker.rs`, `src/checks.rs`, `src/quota.rs`, `src/notices.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`, `backend/crates/config/src/settings_publishing.rs` and `backend/crates/data/src/app_settings.rs` (doc comments: the `[publishing]` keys are no longer "Reserved", and `live = false` parks live deliveries with `live_publish_disabled` while dry runs still run; it never "forces dry runs")
- Test: `backend/crates/publishing/tests/checks.rs`, shared helpers in `backend/crates/publishing/tests/support/mod.rs`

**Interfaces:**
- Consumes: Task 6.
- Produces:
  - `worker::run_publish(service: &PublishingService, job: PublishDelivery) -> Result<(), JobError>`; in this task it claims (`DeliveryRepo::claim` with `settings.lease`; `None` → `Ok(())`), runs the checks, reserves quota, revalidates, and stops just before `StartRemote` (Task 8 continues from there).
  - `checks::park_reason(conn, &ClaimedDelivery, live_switch: bool, available: Availability) -> Result<Option<ParkReason>, DataError>`, in plan order: owner not `active` → `OwnerDisabled`; account not `active` → `AccountNeedsReconnect`; agent not in `compiled_platforms` → `PlatformUnavailable`; platform disabled → `PlatformDisabled`; live delivery with `app_settings.publishing.live` false (read through `AppSettingsRepo`, the runtime switch) → `LivePublishDisabled`. Dry-run deliveries skip the live check only. A compiled, enabled platform that `bound_client` still refuses (for example, a missing deployment config) is not parked: the cause is something `unpark` cannot see, so parking would loop. The delivery ends `Failed` through `RevalidationFailed`, with `last_error {"code": "platform_unavailable"}` and `Notice::DeliveryFailed`, and the owner retries once the admin fixes the config.
  - `quota::reserve(conn, &ClaimedDelivery, &AgentDescriptor, op: QuotaOp, config: &Value, now) -> Result<Reservation, DataError>` with `QuotaOp { Publish, Comment }`, `Reservation { Reserved(Vec<ReservedUnit>), Exhausted { retry_at: DateTime<Utc> } }` (`retry_at` = the exhausted bucket's next period start in its reset time zone; the latest one when several are exhausted), `quota::release(conn, &[ReservedUnit])`. One unit per descriptor bucket with a cost for the op; `period_start` from the bucket's `reset_time_zone` (via `chrono-tz`) and period; limit from `[platforms.<id>].quota.<bucket>` when the config sets it, else `default_limit`. Dry-run reserves nothing.
  - `notices::delivery_notice(conn, &ClaimedDelivery or DeliveryId) -> Result<DeliveryNotice, DataError>` (labels read from `connected_accounts`), used with `Notifier::record` and `Recipients::Workspace { owner, actor: job.created_by }`.
  - Parking: `DeliveryRepo::apply(Fence { expected: Claimed }, DeliveryEvent::Park(reason), ..)`; a live delivery records `Notice::DeliveryParked` in the same transaction. Quota exhausted parks `QuotaExceeded` with `TransitionData.next_attempt_at = retry_at`, so `Sweep::unpark` leaves it until the period rolls over. Revalidation (`validate_target` + the client's `validate` with the assembled assets, or `AssembleError`) failing → `RevalidationFailed` with the violations (codes and paths only) in `last_error`, plus `Notice::DeliveryFailed`. The units reserved just before are released in the same transaction, since no remote call happened.

- [ ] **Step 1: Write failing tests** (all with the `mock` agent through `bound_client`): `stale_dispatch_finds_nothing` (wrong `dispatch_gen`); `not_due_finds_nothing`; `disabled_owner_parks_owner_disabled`; `needs_reconnect_parks`; `uncompiled_platform_parks_platform_unavailable`; `disabled_platform_parks`; `kill_switch_parks_live_but_not_dry_run`; `parked_live_delivery_notifies_owner_once`; `exhausted_bucket_parks_quota_exceeded`; `app_bucket_shared_across_two_owners` (two owners, app bucket limit 1: second parks); `revalidation_failure_fails_with_codes_only`; `deleted_media_fails_revalidation`; `parking_never_degrades_to_dry_run` (live delivery parked, `delivery_attempts` empty, mock publish count 0); `quota_park_waits_for_the_period_and_notifies_once` (Review Focus 9: `next_attempt_at` = the bucket's next period start in its reset zone; one `delivery_parked` row); `revalidation_failure_releases_quota`; `unconfigured_platform_fails_without_parking`; `dry_run_records_no_notifications` (a dry-run delivery parked by a disabled platform records no notice).
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test checks` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): claim deliveries, park on failed checks, and reserve quota`.

### Task 8: Remote call, outcomes, retries, progress, and late receipts

**Files:**
- Create: `backend/crates/publishing/src/outcome.rs`, `src/progress.rs`, `src/backoff.rs`
- Modify: `backend/crates/publishing/src/worker.rs`, `backend/crates/agents/mock/src/client.rs` and `src/script.rs` (test controls below)
- Test: `backend/crates/publishing/tests/publish.rs`, `backend/crates/agents/mock/tests/` (script test for the new controls)

**Interfaces:**
- Consumes: Task 7.
- Produces:
  - Mock test controls. The mock's live publish is one step today: no chunks, no `save_container_ref`, and one `cancel_requested` check. Tasks 8 and 12 need two additions on `MockScript`:
    - `set_publish_steps(n)`: a live publish uploads `n` chunks, calls `progress.save_container_ref` after each, checks `cancel_requested()` between chunks (returning `Cancelled`), and resumes from a given `container_ref`.
    - `hold_publishes() -> PublishGate`: live publish calls wait at the gate before they land, until the test releases them.
  - Worker continuation: `apply(StartRemote)` → `AttemptRepo::start` → spawn the lease renewer (`renew_lease` every `lease / 3`; a `false` renewal sets the progress's `lost` flag) → call `client.publish` (operation `publish`) or `client.commenter().comment` (operation `comment`; no `Commenter` → `PermanentFailure` with code `capability_unsupported`) with `PublishRequest { variant, mode: Live|DryRun, native_publish_at: job native schedule ? scheduled_at : None, container_ref, idempotency_key }` → `AttemptRepo::finish` → map the result.
  - `progress::DeliveryProgress` implements `PublishProgress`: `save_container_ref` writes `DeliveryRepo::set_container_ref` under the fence (a stale fence returns `AgentError::Cancelled`); `cancel_requested()` is true when the lease was lost (`lost`), when the renewer saw the delivery's `cancel_requested_at` set, or when `shutdown` is cancelled (Task 12 uses the last two). A worker that lost its lease therefore stops at the agent's next checkpoint instead of finishing a multi-step upload the sweep already handed on.
  - `outcome::map(result: Result<PublishOutcome, AgentError>, attempt_count: i32, max_attempts: u32, now, cancel: CancelCause) -> Mapped` with `Mapped { event: DeliveryEvent, data: TransitionData, follow_up: FollowUp, notice: Option<NotificationKind> }`, `FollowUp { None, Poll { at }, Reconcile, Redispatch, NeedsReconnect }`, `CancelCause { None, User, Shutdown, LeaseLost }`:
    - `Published(r)` → `Published`; `Processing(r)` → `Accepted`, follow-up `Poll { at: now + 15s }` (or `remote_publish_at` when it is later, for native schedules), first poll job `poll = 0`; `Simulated` → `Simulated`.
    - `RetryClass::Retryable` / `RetryAfter(d)` → `RetryableFailure` with `next_attempt_at` from `backoff::retry_delay(attempt)` (or `d`), `count_attempt: true`; when `attempt_count + 1 >= max_attempts` → `RetryBudgetExhausted` (`DeadLetter`, notice `DeliveryDeadLettered`), also with `count_attempt: true`.
    - `Permanent` → `PermanentFailure` (notice `DeliveryFailed`); `NeedsReconnect` → `Park(AccountNeedsReconnect)` + `FollowUp::NeedsReconnect` (the worker calls `AccountService::mark_needs_reconnect(OwnerScope::own(owner), grant, <the system audit context the oauth jobs already use>)` after its commit); `QuotaExceeded` → `Park(QuotaExceeded)` with `next_attempt_at` = the error's `retry_after` (else the bucket's next period start; never left unset, see Global Constraints); `Ambiguous` → `Ambiguous` + `Reconcile` (notice `DeliveryOutcomeUnknown`); `Cancelled` with `CancelCause::User` → `Cancel`, with `Shutdown` → `Shutdown` (`Pending`, generation + 1, redispatched), with `LeaseLost` → nothing is written (the sweep owns the delivery; any write would be `Stale`).
    - `last_error` holds `{ "code": <AgentError code>, "class": <retry class> }`, never platform message text.
  - Release reserved quota when the result proves no remote call happened (`Cancelled` before the first mutating call, `Authentication` before send).
  - `Applied::Stale` with a `Published` receipt → `DeliveryRepo::record_late_receipt` (it acts only while the delivery is `outcome_unknown`). A `Processing` receipt is not recorded: the platform has not confirmed the post, and `record_late_receipt` would mark it `Published`. Reconcile finds it through `container_ref` or the idempotency key. Anything else is dropped silently.
  - `backoff::retry_delay(attempt: i32) -> Duration` and `backoff::poll_delay(poll: i32) -> Duration` with the Global Constraints values; jitter from `rand`.
  - Notices, for live deliveries only, on `DeliveryPublished` (info), `DeliveryFailed`, `DeliveryDeadLettered`, `DeliveryOutcomeUnknown`, and `JobPartial` when `refresh_job_status` returns `Partial` for a live job. Audit events `delivery_published`, `delivery_dead_lettered`, and `delivery_outcome_unknown` (IDs only; kinds from Task 6).

- [ ] **Step 1: Write failing tests** with the mock's scripted `outcome` option: `published_records_receipt_and_notifies_info`; `processing_schedules_a_poll`; `retryable_failure_backs_off_with_new_generation`; `retry_budget_ends_in_dead_letter` (`max_attempts = 2`); `permanent_failure_fails`; `needs_reconnect_parks_and_marks_the_grant`; `rate_limited_uses_retry_after`; `outcome_unknown_enqueues_reconcile`; `dry_run_simulates_and_leaves_post_status` (`posts.status` unchanged, attempt recorded with the planned action); `comment_post_goes_through_commenter`; `container_ref_saved_before_next_step_and_reused_on_retry`; `stalled_worker_late_success_is_recorded_once` (Review Focus 1a: renewer stopped, lease expired, sweep marks `OutcomeUnknown`, worker then returns `Published`: one `Published` row with that receipt, mock publish count 1, the reconcile job exits); `late_success_after_reconcile_absent_publishes_once` (Review Focus 1b: the mock holds the stalled call before it lands, reconcile answers `absent`, the redispatched run publishes, then the held call lands with the same key: mock post count 1, delivery `Published` with that post, the stalled write `Stale`); `late_processing_receipt_is_not_recorded_as_published`; `lost_lease_stops_at_next_checkpoint` (multi-step script, renewal refused: the agent returns `Cancelled` at its next checkpoint, nothing written); `duplicate_apalis_job_publishes_once` (two `PublishDelivery` with the same generation run concurrently: mock count 1); `concurrent_workers_publish_once` (10 tasks); `console_retry_of_every_publishing_type_publishes_nothing_twice` (re-enqueue each payload after success: mock count unchanged); `retry_after_edit_publishes_the_snapshot` (post edited after job creation: mock receives the snapshot text); `partial_job_notifies` (two targets, one permanent failure); `quota_released_when_no_call_was_made`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test publish` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): publish through agents with fenced outcomes, retries, and late receipts`.

### Task 9: Processing polls, timeouts, native schedules, and reconcile

**Files:**
- Create: `backend/crates/publishing/src/poll.rs`, `src/reconcile.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`, `backend/crates/agents/mock/src/client.rs` and `src/script.rs` (scripted reconcile answers), `backend/crates/data/migrations/0001_init.sql` (`deliveries.check_seq`), `backend/crates/data/src/publishing/deliveries.rs` (`take_check`, `apply` resets `check_seq`)
- Test: `backend/crates/publishing/tests/processing.rs`, `backend/crates/agents/mock/tests/` (script test), `backend/crates/data/tests/publishing_deliveries.rs` (`take_check`)

**Interfaces:**
- Consumes: Task 8.
- Produces:
  - Mock: post option `reconcile` with values `found` (default when the earlier publish landed), `absent`, `unresolvable` (returns `ReconcileOutcome::Unresolvable`), `unsupported` (returns `AgentError::Unsupported`), plus the same for comments through `reconcile_comment`.
  - **One chain per delivery.** `Processing` and `OutcomeUnknown` never bump the generation. Without a guard, two poll or reconcile jobs with the same generation would each re-enqueue themselves, for example after an at-least-once redelivery. The guard has three parts:
    - New column `deliveries.check_seq INT NOT NULL DEFAULT 0`. `DeliveryRepo::apply` resets it to 0 whenever the state changes.
    - `DeliveryRepo::take_check(conn, fence, seq: i32) -> Result<bool, DataError>` runs `UPDATE … SET check_seq = check_seq + 1 WHERE <fence> AND check_seq = $seq`.
    - A poll job with `poll = k`, or a reconcile job with `attempt = k`, proceeds only when `take_check(k)` is true, and enqueues its successor with `k + 1`. A duplicate of the same `k` finds `false` and exits. The first poll (Task 8) and the first reconcile (Task 8's `Ambiguous`, Task 10's sweep) start at 0.
  - `poll::run_poll(service, PollDeliveryStatus) -> Result<(), JobError>`: reads the delivery (`Processing`, matching generation, else exit); a native-scheduled delivery whose `remote_publish_at` is in the future and whose upload was accepted re-enqueues itself at `remote_publish_at`; otherwise `client.status(account, receipt)`: `Published` → `Published`; still `Processing` → re-enqueue with `poll + 1` at `now + poll_delay(poll + 1)`; `Permanent` → `PermanentFailure`; transport errors → re-enqueue. Past `processing_timeout` (from `remote_publish_at` for native schedules, else from the attempt start) → `ProcessingTimeout` (`OutcomeUnknown`) + reconcile. Direct deliveries are never polled here.
  - `reconcile::run_reconcile(service, ReconcileDelivery) -> Result<(), JobError>`: `OutcomeUnknown` with matching generation, else exit. Calls `reconcile` (or `reconcile_comment`) with the last attempt's start time and `container_ref`. `Found` → `ReconcileFound` with the receipt; `Absent` → `ReconcileAbsent` (redispatch, generation + 1); `Unresolvable` or `Unsupported` → `ReconcileUnresolvable` (`Parked(OutcomeUnresolvable)`, notice `DeliveryParked`); retryable errors re-enqueue with `attempt + 1` at `poll_delay(attempt + 1)` until `processing_timeout` since the attempt started, then `ReconcileUnresolvable`.

- [ ] **Step 1: Write failing tests**: `poll_publishes_when_platform_finishes`; `poll_backs_off_to_fifteen_minutes`; `processing_timeout_moves_to_outcome_unknown`; `native_schedule_timeout_counts_from_remote_publish_at`; `native_upload_waits_until_publish_time`; `reconcile_found_publishes`; `reconcile_absent_redispatches_once`; `reconcile_unresolvable_parks_and_notifies`; `reconcile_unsupported_parks` (spec gap: mock never returned it before); `comment_reconcile_uses_reconcile_comment`; `stale_reconcile_job_exits`; `duplicate_poll_job_does_not_fork_the_chain` (two `PollDeliveryStatus` with the same `poll`: one status call, one successor); `duplicate_reconcile_job_does_not_fork_the_chain`; data test `take_check_admits_one_caller_per_seq_and_resets_on_state_change`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test processing` and the mock tests — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): poll processing deliveries and reconcile unknown outcomes`.

### Task 10: Due sweep and account preflight

**Files:**
- Create: `backend/crates/publishing/src/sweep.rs`, `src/preflight.rs`
- Modify: `backend/crates/publishing/src/jobs.rs`, `backend/crates/data/src/publishing/sweep.rs` (`expired_in_flight` returns `Vec<Dispatch>` instead of `Vec<DeliveryId>`, so the reconcile job carries the generation; update `backend/crates/data/tests/publishing_sweep.rs` and the job map in `data/src/publishing/mod.rs`; plus the preflight query if missing: deliveries due within an hour on accounts not checked in the last hour)
- Test: `backend/crates/publishing/tests/sweep.rs`, `backend/crates/publishing/tests/preflight.rs`

**Interfaces:**
- Consumes: Tasks 8 and 9.
- Produces:
  - `sweep::run_sweep(service) -> Result<SweepReport, JobError>` with `SweepReport { requeued: usize, reclaimed: usize, unknown: usize, orphaned_direct: usize, unparked: usize, stale: usize }`, in one pass with `limit = 500` per query: `overdue_pending(now, sweep_grace)` and `expired_claims` → enqueue `PublishDelivery` for each `Dispatch`; `expired_in_flight` → enqueue `ReconcileDelivery { attempt: 0 }` with the returned generation (multi-step agents resume from `container_ref`); `orphaned_direct(now, sweep_grace)` → enqueue `DirectSendBatch`; `unpark(UnparkInput { now, park_grace, compiled_platforms, limit })` → enqueue `resumed`, and for each `needs_owner` record `Notice::DeliveryStale` (the B2b query already sets `parked_notified_at`; the worker never sets it at park time). Each enqueue runs in the transaction that produced the dispatch: the sweep opens a transaction per duty, passes it to the `Sweep` function (whose own `begin` becomes a savepoint), enqueues, and commits.
  - `preflight::run_preflight(service) -> Result<usize, JobError>`: accounts with a live delivery due within 1 hour and `last_checked_at` older than 1 hour (or null); `client.account_info`; active → `AccountRepo::mark_checked`; not active or `Authentication`/`InvalidCredentials` → `AccountService::mark_needs_reconnect` (Task 14 adds its notification); transport errors skip the account until the next run. No database connection is held across a platform call.

- [ ] **Step 1: Write failing tests**: `overdue_pending_is_requeued_with_new_generation`; `expired_claim_returns_to_pending_and_requeues`; `expired_in_flight_becomes_unknown_and_reconciles`; `orphaned_direct_batch_is_requeued_once_per_grace`; `unpark_respects_park_grace_for_every_reason` (Review Focus 2: for each of `owner_disabled`, `account_needs_reconnect`, `platform_unavailable`, `platform_disabled`, `live_publish_disabled`, `quota_exceeded`, cleared inside grace → resumed; outside → stays parked, one `delivery_stale` notice across two sweeps); `outcome_unresolvable_never_unparks`; `parked_delivery_stays_parked_while_its_cause_persists` (Review Focus 9: for each reason except `outcome_unresolvable`, cause still present across two sweeps: still parked, no new notice, no enqueue); `expired_in_flight_reconcile_carries_the_generation`; `lost_queue_entry_delays_but_never_duplicates` (delete the apalis row; sweep republishes once); `preflight_detects_revoked_credential` (mock token `invalid` → grant `needs_reconnect`, delivery later parks); `preflight_skips_recently_checked_accounts`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test sweep --test preflight` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): add the due sweep and account preflight`.

### Task 11: Direct deliveries

**Files:**
- Create: `backend/crates/publishing/src/direct.rs`
- Modify: `backend/crates/publishing/src/worker.rs` (operation `direct`), `src/jobs.rs`, `backend/crates/data/migrations/0001_init.sql` (`direct_sends.tries`), `backend/crates/data/src/publishing/direct.rs` (`DirectRepo::requeue`, and `audience_count` if absent)
- Test: `backend/crates/publishing/tests/direct.rs`, `backend/crates/data/tests/publishing_direct.rs` (`requeue`)

**Interfaces:**
- Consumes: Tasks 7 and 8; `DirectRepo`, `UserLimitsRepo::get`, `postit_data::publishing::quota::EMAIL_CAP_BUCKET`.
- Produces:
  - Worker, operation `direct`: after the checks, `StartRemote`, then the audience from the variant's options (`lists: [ContactListId]`, `tags: [String]`, as `Audience`). Dry-run: count with the materialize query's count form (add `DirectRepo::audience_count(conn, owner, &Audience)` if absent), record it in the attempt, end `Simulated`, write nothing to `direct_sends`. Live: `DirectRepo::materialize`, `apply(Accepted)` (`Processing`, `direct_counts.audience` set), enqueue `DirectSendBatch { delivery, dispatch_gen }`, all in one transaction.
  - `direct::run_batch(service, DirectSendBatch) -> Result<(), JobError>`: `claim_batch(Fence { expected: Processing }, size = settings.email.batch_size, lease, cap)` with `cap = Some((&AccountQuotaKey { bucket: EMAIL_CAP_BUCKET, period_start: owner's local date }, user_limits.email_daily_cap))` (passed through as is; the repo treats a limit of 0 as unlimited). `Rows` → load recipient fields (`contacts` email, first and last name, custom fields, locale) for those rows, build `DirectRecipient`s, call `client.direct().send_batch(account, recipients, variant)`, renew the batch lease during the call; map each result: `Sent` → `DirectResult::Sent`, `Bounced { permanent: true }` → `Bounced` + a suppression through the existing `SuppressionRepo::add_hash` in `data/src/contacts.rs` with the bounced reason, `Bounced { permanent: false }` and `Failed { retryable: true }` → `DirectRepo::requeue` while the row's `tries < 2`, else `Failed` (a third failed try ends it); `Failed { retryable: false }` → `Failed`.
  - Soft-failure retries need what `direct_sends` lacks today: `DirectRepo::record` is final (it clears the lease and sets an end state), and no column counts tries.
    - New column `direct_sends.tries SMALLINT NOT NULL DEFAULT 0`.
    - New `DirectRepo::requeue(conn, id: DirectSendId, batch: DirectBatchId, now) -> Result<bool, DataError>`: the row stays `queued`, its batch and lease are cleared, and `tries + 1` is written. It is refused (`false`) when that batch no longer holds the row, the same condition `record` uses.
    - A requeued row's cap unit is not given back: the provider may have counted the try, which matches `claim_batch`'s over-count rule, and the row's next claim reserves again. In the same transaction as the last `record`: `finish_if_done` (all terminal → `Published` with counts, notice `DeliveryPublished`); otherwise enqueue the next `DirectSendBatch`. `CapReached` → `resume_at` = the next local midnight in the owner's time zone, `wait_for_cap`, enqueue at `resume_at`. `Empty` → `finish_if_done`. `Stale` → exit.
  - A whole-batch `AgentError` (transport) leaves the rows leased; their lease expiry returns them to `queued` and the sweep's `orphaned_direct` re-enqueues.

- [ ] **Step 1: Write failing tests** with `mock_direct` and contacts inserted directly: `audience_materialized_once_and_fixed` (adding a contact after materialization does not add a row); `suppressed_and_unsubscribed_are_skipped`; `batches_send_every_recipient_once`; `cap_boundary_waits_for_next_local_day` (cap 3, audience 5: 3 sent, delivery `Processing`, next batch at local midnight, then 2 more); `bounce_5xx_suppresses`; `soft_failure_retries_within_budget` (soft failure twice then sent: `sent`; three soft failures: `failed`, `tries = 2`); data test `requeue_returns_the_row_and_counts_the_try` (and refuses a row another batch holds); `crash_mid_batch_resumes_without_resending_terminal_rows`; `direct_dry_run_reports_count_and_writes_nothing`; `campaign_with_some_failures_is_published_with_counts`; `processing_timeout_exempts_direct_with_queued_rows`.
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test direct` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): send direct deliveries in capped batches`.

### Task 12: Service API, cancellation checkpoints, and graceful shutdown

**Files:**
- Create: `backend/crates/publishing/src/service.rs`, `src/error.rs`
- Modify: `backend/crates/publishing/src/worker.rs`, `src/progress.rs`, `backend/crates/server/src/lib.rs` (pass the shutdown token into `PublishingDeps`)
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
  - The service calls write their audit events with the kinds Task 6 added.

- [ ] **Step 1: Write failing tests**: `stale_if_match_is_refused`; `live_publish_needs_confirm`; `schedule_up_to_a_minute_past_is_now`; `mixed_native_and_postit_targets_in_one_job`; `edit_delegate_can_dry_run_but_not_publish_live`; `limited_delegate_cannot_target_outside_limit`; `reschedule_moves_pending_and_native_uploads`; `reschedule_refused_while_in_flight`; `cancel_at_each_safe_checkpoint` (before first mutating call; between chunks with the mock's multi-step script); `cancel_after_final_call_finishes`; `native_cancel_with_and_without_agent_delete`; `outcome_unknown_cannot_be_cancelled`; `retry_requires_confirm_and_acknowledgement`; `retry_refused_while_post_has_another_live_job` (`job_active`); `resolve_from_dead_letter`; `jobs_survive_delegation_revocation` (delegate creates, grant revoked, delivery still publishes); `shutdown_at_checkpoint_requeues_and_publishes_once` (Review Focus 6); `shutdown_after_final_call_finishes_within_grace`; isolation matrix for every service call (owner, `view`/`edit`/`publish` delegates, limited delegate, unrelated user).
- [ ] **Step 2: Run** `cargo test -p postit-publishing --test service --test shutdown` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(publishing): add the publishing service, cancellation checkpoints, and graceful shutdown`.

### Task 13: Deletion steps and the disconnect hook

**Files:**
- Create: `backend/crates/publishing/src/deletion.rs`, `src/disconnect.rs`, `backend/crates/media/src/deletion.rs`
- Modify: `backend/crates/oauth/src/disconnect.rs`, `backend/crates/oauth/src/service.rs` (`AccountServiceDeps.disconnect_hook: Option<Arc<dyn DisconnectHook>>`), `backend/crates/oauth/src/lib.rs`, `backend/crates/data/src/publishing/deliveries.rs` (`cancel_for_account` also cancels `processing` direct deliveries), `backend/crates/media/src/storage.rs` (`FsMediaStore::remove_owner_files`), `backend/crates/data/src/media.rs` (the owner's unfinished upload IDs, if no query lists them), `backend/crates/server/src/compose.rs` (step order and hook injection)
- Test: `backend/crates/publishing/tests/deletion.rs`, `backend/crates/publishing/tests/disconnect.rs`, `backend/crates/media/tests/deletion.rs`, `backend/crates/oauth/tests/disconnect.rs` (extend), `backend/crates/data/tests/publishing_actions.rs` (where `cancel_for_account` is tested)

**Interfaces:**
- Produces:
  - In `postit-oauth`: `#[async_trait] pub trait DisconnectHook: Send + Sync { async fn before_disconnect(&self, owner: UserId, account: AccountId) -> Result<NativeCleanup, AccountError>; }` with `NativeCleanup { deleted: u32, remaining: u32 }`. `disconnect` runs in this order:
    1. It checks `DeliveryRepo::account_has_in_flight` first and refuses with `JobActive` before touching the platform.
    2. It calls the hook while the credentials are still valid.
    3. It runs its transaction, which checks in-flight again as today.

    So a refused disconnect leaves the platform untouched, except in the narrow race where a claim lands between steps 1 and 3. `DisconnectOutcome.native_scheduled_remaining` = `remaining`. Without a hook (tests, API role without publishing), behavior is today's.
  - `postit_publishing::PublishingDisconnect` implements it, for native-scheduled uploads only: it deletes each native-scheduled upload of the account through `NativeScheduler::delete` and cancels it; uploads the agent cannot delete stay `remaining`.
  - Stopping a running campaign happens inside the disconnect transaction, not in the hook. `DeliveryRepo::cancel_for_account` also takes the account's `processing` direct deliveries and cancels them through `apply(Cancel)`, which already cancels the queued rows and stores the counts. The campaign ends `cancelled`, with its sent rows kept.
  - `postit_publishing::PublishingDeletionStep` (`name() == "publishing"`): cancels the user's pending and parked deliveries and running direct deliveries, requests cancel on in-flight ones, deletes native-scheduled uploads (as the hook does), and returns `JobError::Retry` while any of the user's deliveries is `claimed` or `in_flight`. Idempotent.
  - `postit_media::MediaDeletionStep` (`name() == "media"`) works by owner, not by listing rows. Storage keys are `{owner}/{yyyy}/{mm}/{asset}` plus `.thumb.jpg`, and upload parts live in `PARTS_DIR` by upload ID.
    - It removes the owner's whole key tree with `FsMediaStore::remove_owner_files(owner)` (`remove_dir_all` on `{root}/{owner}/`; a missing directory means done). That covers originals and thumbnails.
    - It removes the part files of the owner's unfinished uploads (`part_path(id)`; a missing file means done).
    - This closes the B4 deferred entry.
  - Server registers steps in order: publishing, oauth (exists), media.

- [ ] **Step 1: Write failing tests**: `disconnect_deletes_native_uploads_through_the_agent`; `disconnect_lists_uploads_the_agent_cannot_delete`; `disconnect_stops_a_running_campaign` (in the disconnect transaction: delivery `cancelled`, queued rows cancelled, sent rows stay sent, counts stored); `disconnect_refused_while_in_flight_leaves_the_platform_untouched` (no `NativeScheduler::delete` call); `disconnect_keeps_history`; `delete_user_with_pending_in_flight_and_native_deliveries` (retries until in-flight finishes, then completes); `publishing_step_is_idempotent`; `media_step_removes_files_and_tolerates_missing` (originals, thumbnails, and an unfinished upload's part; a second run is a no-op); data test `cancel_for_account_cancels_running_direct_delivery`; `steps_run_in_order` (recording fakes in the identity handler test).
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
  - Two functions set a grant to `needs_reconnect` today:
    - `mark_needs_reconnect` (`oauth/src/manage.rs:188`), used by the worker and preflight;
    - `mark_reconnect` (`oauth/src/refresh.rs:226`), used by the `invalid_grant` refresh path and the `reconnect_by` deadline path in `run_refresh_credentials`.

    Both record the notices in their transaction, one per affected account (labels), with `Recipients::Workspace { owner, actor: None }`. `mark_reconnect` gains a `cause: ReconnectCause { Refused, DeadlinePassed }`: `Refused` records `Notice::AccountNeedsReconnect`, and `DeadlinePassed` records `Notice::ReconnectByPassed` instead. So each event yields one critical notice per account, never both.
  - Platform deauthorization records `Notice::AccountRevokedByPlatform` per account.
  - `refresh_credentials` stages: for active grants with `reconnect_by`: within 7 days and `reconnect_warned = 'none'` → `Notice::ReconnectReminder { stage: SevenDays }`, set `7d`; within 1 day and warned `none|7d` → `OneDay`, set `1d`; passed → existing `mark_reconnect` with `DeadlinePassed` (notice `ReconnectByPassed` only), set `expired`. Each stage once; a grant first seen inside 1 day gets only the `OneDay` reminder.

- [ ] **Step 1: Write failing tests**: `needs_reconnect_notifies_owner_per_account`; `invalid_grant_on_refresh_notifies`; `deauthorize_notifies_revoked`; `reminders_at_seven_days_and_one_day_once_each`; `grant_first_seen_inside_one_day_gets_one_reminder`; `reconnect_by_passed_notifies_once_per_account` (one `reconnect_by_passed` row, no `account_needs_reconnect` beside it); `delegates_are_not_told_about_reconnects` (no actor on these events).
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
  - `record` fan-out, by row type:
    - **Critical rows.** A critical row is email-wanted when the recipient's `email_notifications` is on and `NotificationSwitches::email(kind)` is on.
    - **`new_comments` rows.** These are email-wanted when the recipient's `comment_digest` and `email_notifications` are on; the digest takes them in Task 18.
    - **Every other row** is inserted with `email_wanted: false` (`emailed_at = created_at`), so it never appears in an email and never waits for one.
    - **One alert job per wanted critical row.** For each email-wanted critical row, enqueue `send_email { kind: NotificationAlert, recipient, params: MailParams::None }` at `now + ALERT_DELAY`. The first job to run takes every unemailed critical row of the recipient, and the later ones find nothing and skip.
    - **No "an alert is already pending" check.** It cannot see rows a running job has taken but not yet committed, so a row recorded during a send would never be emailed.
    - **Exhausted jobs.** A job that exhausts its retries leaves its rows unemailed, and the next critical row's job takes them too.
  - `NotificationAlertLoader` implements `MailContextLoader`: `load` calls `take_unemailed_critical(recipient, now)` (marking happens here, inside the handler's transaction, which rolls back if SMTP fails); empty → `Skip`; otherwise renders one email listing every notice (one line each from its kind and labels, in the recipient's locale) with a link to `{app.public_url}/`. `mark_sent` is a no-op for this kind. *(The loader marks in `load` because `mark_sent` does not receive which rows were included; the transaction makes it equivalent.)*
  - Subject `{ $count -> [one] Something needs your attention on postit *[other] { $count } things need your attention on postit }`; Arabic equivalents in `ar/notify.ftl`. Lines per kind keyed `notice-<kind>` with `$account`, `$platform`, `$count`, `$reason` arguments. Templates on the shared `layout.html`.

- [ ] **Step 1: Write failing tests**: `ten_failures_send_one_email` (Review Focus 3: ten `DeliveryFailed` notices for one user in one minute → ten `send_email` jobs; the first renders ten lines and marks all ten rows, and the other nine skip; one email sent); `opted_out_kind_never_blocks_later_alerts` (Review Focus 8: email off for `delivery_parked`, a parked notice then a failed notice: one email with only the failed line, both rows settled); `row_recorded_during_a_send_is_emailed_next` (Review Focus 8: a row committed while the first job holds its transaction is emailed by its own job); `email_switch_off_sends_nothing`; `master_switch_off_sends_nothing`; `info_kinds_never_email`; `smtp_failure_leaves_rows_unemailed` (handler transaction rolled back); `later_job_skips_when_nothing_left`; snapshots `alert_en`, `alert_ar` (`dir="rtl"`); no address, token, or post text in the rendered email beyond the recipient's own address header (leak harness).
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
  - `NotificationDigest` recurring job (`"notification_digest"`, `Queue::Maintenance`, schedule `notification_digest`, `RetryPolicy::Backoff` like the other crons): for every user with `comment_digest = true` and `email_notifications = true`, computes the local time in `user_preferences.timezone` (`chrono_tz`); when the local hour is `DIGEST_HOUR` and `last_comment_digest_at` is before today's local midnight (or null), enqueues `send_email { kind: CommentDigest, recipient, params: None }`. The cron fires at minute 0 UTC, so in a zone with a 30- or 45-minute offset the run falls at 08:30 or 08:45 local; "local hour 8" is the rule, not 08:00 sharp.
  - `CommentDigestLoader`: `load` calls `take_undigested_comments(user, now)` (marks the rows inside the handler's transaction, as the alert loader does; rolled back if SMTP fails); empty → `Skip`; otherwise one line per account with the summed count. `mark_sent` sets `last_comment_digest_at = at`, which only gates the once-per-local-day check. Which rows go out is decided by the row marks, so a row whose transaction commits after the read goes in the next digest instead of being skipped.
  - Tests drive it with `Notice::NewComments` rows inserted through `Notifier::record` (no producer until E2).

- [ ] **Step 1: Write failing tests**: `digest_runs_at_local_eight_for_offset_zones` (Review Focus 7: `Asia/Riyadh`, `Asia/Kolkata`, `America/New_York` across a DST change: exactly one enqueue per local day, in local hour 8 — 08:00 in Riyadh and New York, 08:30 in Kolkata); `digest_off_sends_nothing`; `late_committed_comment_is_in_the_next_digest`; `comments_recorded_with_digest_off_are_never_digested`; `no_comments_skips`; `counts_are_summed_per_account`; snapshots `digest_en`, `digest_ar`.
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
  - **Phase-start check:** pick the Web Push implementation. A crate qualifies only if it meets all of these:
    - It builds the encrypted body (RFC 8291 `aes128gcm`) and the VAPID `Authorization` header (RFC 8292) without sending.
    - It pulls in no OpenSSL. After adding it, `cargo tree -i openssl-sys` from `backend/` must be empty; `web-push` depends on `ece`, whose default backend is OpenSSL, so check whether a pure-Rust configuration exists.
    - Its license is in `deny.toml`.
    - It is maintained.
    - It has no Arabic-derived name.

    Otherwise implement it with the workspace's `p256` (ECDH, ECDSA), `hkdf`, and `jsonwebtoken` (ES256), plus RustCrypto `aes-gcm` (MIT/Apache-2.0), keeping the RFC 8291 test vectors. Record the choice in the commit message.
  - `vapid::VapidKeys::from_settings(&PushSettings) -> Result<Option<VapidKeys>, NotifyError>` (`None` when disabled); `VapidKeys::public_key_b64url(&self) -> String` (uncompressed P-256, base64url without padding). `cargo xtask vapid-keygen` prints `POSTIT__PUSH__VAPID_PRIVATE_KEY=<b64url>` and the public key.
  - `PushSend { notification: Uuid, endpoint: Uuid }` job (`"push_send"`, `Queue::Default`, `RetryPolicy::Backoff { max_attempts: Some(5), initial: 30s, max: 30min }`, console id fields `notification`, `endpoint`). Handler: load endpoint (gone → done) and notification (gone → done), render title and body in the recipient's locale from `notify.ftl` (`push-title-<kind>`, `push-body-<kind>`), payload JSON `{ "id", "kind", "title", "body", "tag" }`, with `tag` set as follows:
    - for delivery and job kinds, the job ID, so one job's pushes collapse on the device;
    - otherwise, the notification ID.

    Encrypt it and send it through the public client with `TTL: 86400`, `Urgency: high` for critical kinds (`normal` otherwise), and `Topic` (RFC 8030) set to the base64url of the tag's 16 UUID bytes (22 characters, within the 32 allowed), so the push service keeps only the latest undelivered message per job. `2xx` → `mark_success`, `mark_pushed`; `404`/`410` → `delete_by_id`, done; `429`/`5xx`/transport → `JobError::Retry`; other `4xx` → done with a warning (no endpoint in the log).
  - `record` fan-out: when `push_enabled` and `NotificationSwitches::push(kind)`, enqueue one `PushSend` per endpoint of the recipient.
  - Routes (self, `owner_only` under act-as): `GET /api/v1/push/vapid-public-key` → `{ key }` or 404 `push_disabled`; `POST /api/v1/me/push-endpoints` with `{ endpoint, p256dh, auth, platform: android|ios|web, device_label }` → 201 `{ id }` (endpoint must be `https`, or `http` only when `push.allow_private_endpoints` is on, for self-hosted UnifiedPush distributors on a LAN; `p256dh` a 65-byte uncompressed point and `auth` 16 bytes, both base64url; otherwise 422 `push_endpoint_invalid`; 404 `push_disabled` when off); `DELETE /api/v1/me/push-endpoints/{id}` → 204 (another user's ID → 404). Endpoint hash = SHA-256 of the endpoint URL (`emixcrypto`).
  - The address policy applies at send time (a private endpoint fails as permanent unless `allow_private_endpoints`).

- [ ] **Step 1: Write failing tests**: `rfc8291_test_vector` (encrypt with the RFC's fixed keys and salt; compare the body); `vapid_header_shape` (`vapid t=<jwt>, k=<key>`, JWT `aud` = endpoint origin, `exp` ≤ 24h, `sub` = subject); `push_delivers_payload_without_text` (wiremock endpoint; decrypted payload has only id, kind, title, body, tag; no post text); `job_pushes_share_a_topic` (Review Focus 3: two delivery notices of one job carry the same `tag` and `Topic`); `http_endpoint_needs_allow_private` (route: 422 `push_endpoint_invalid` unless allowed); `gone_endpoint_is_deleted_and_others_still_receive` (Review Focus 5: two endpoints, one `410`); `rate_limited_endpoint_retries`; `private_endpoint_refused_unless_allowed`; `push_switch_off_enqueues_nothing`; `push_disabled_enqueues_nothing`; routes: `vapid_key_404_when_disabled`, `register_validates_keys`, `re_register_replaces`, `delete_other_users_endpoint_is_404`, `act_as_is_owner_only`; leak harness: no endpoint URL or key in logs, errors, or problem bodies.
- [ ] **Step 2: Run** `cargo test -p postit-notify --test push` and `cargo test -p postit-api --test push` — FAIL.
- [ ] **Step 3: Implement**; generate the dev key pair with `cargo xtask vapid-keygen` into the extracted dev vault (the maintainer re-packs `vault.7z`); `cargo xtask openapi`.
- [ ] **Step 4: Run** the tests, `cargo xtask openapi --check`, mobile analyze and test — PASS.
- [ ] **Step 5: Commit** `feat(notify): send Web Push with VAPID and manage push endpoints`.

### Task 21: Browser push and notification settings (web)

**Files:**
- Create: `web/public/sw.js`, `web/src/components/ui/switch.tsx` (the kit has no switch yet: a shadcn-style `Switch` on the existing `radix-ui` dependency, with a component test), `web/src/features/me/NotificationSettings.tsx`, `web/src/features/me/NotificationSettings.test.tsx`, `web/src/features/me/useBrowserPush.ts`, `web/src/features/me/usePreferences.ts`, `web/src/features/me/sw.test.ts`
- Modify: `web/src/features/me/ProfilePage.tsx`, `web/src/main.tsx` (register the service worker when supported), `web/src/i18n/en-US/me.json`, `web/src/i18n/ar/me.json`, `backend/crates/server/src/web.rs` (CSP `worker-src 'self'`; `sw.js` served with `Cache-Control: no-cache`), `backend/crates/server/tests/web.rs`
- Test: the web tests above, `backend/crates/server/tests/web.rs`

**Interfaces:**
- Consumes: Tasks 16, 19, 20 (generated `schema.d.ts`).
- Produces:
  - `sw.js` (plain JS, root scope): `push` → `showNotification(title, { body, tag, renotify: true, data: { id } })` (`tag` from the payload, so one job's notices replace each other); `notificationclick` → close, focus an open postit client or open one, navigate to `/?notification=<id>`.
  - `useBrowserPush()` → `{ state: 'unsupported' | 'blocked' | 'off' | 'on' | 'working', enable(), disable() }`. `enable` asks permission once (no prompt when `Notification.permission === 'denied'`), subscribes with `applicationServerKey` from `GET /push/vapid-public-key`, posts `{ endpoint, p256dh, auth, platform: 'web', device_label }` (label `"<browser> on <os>"` from the user agent, fallback `"Browser"`), stores the returned ID per subscription in `localStorage` (try/catch). `disable` unsubscribes and deletes the endpoint. A 404 `push_disabled` shows `unsupported` with a "not enabled on this server" note.
  - `NotificationSettings` on the profile page, built from the D0b kit (`FormField`, `OptionSelect` from `components/kit`, the new `Switch` from `components/ui`): the browser push switch, the master email switch, the digest switch, the time-zone select (IANA list from `Intl.supportedValuesOf('timeZone')`, prefilled with the browser's zone when the stored value is the default `UTC`, saved on change), and a per-kind table of push and email switches (email column only where the API returns `email`). Saves with partial `PUT /me/preferences`.
  - Strings in English and Arabic for every kind name and state; right-to-left layout checked.

- [ ] **Step 1: Write failing tests**: `blocked_permission_shows_blocked_state` (Review Focus 5: mocked `Notification.permission = 'denied'`; no `requestPermission` call); `enable_subscribes_and_registers`; `disable_unsubscribes_and_deletes`; `push_disabled_server_shows_unsupported`; `timezone_prefilled_from_browser_when_default`; `kind_switch_sends_partial_update`; `renders_rtl_in_arabic`; `sw.test.ts`: `push_event_shows_notification`, `click_focuses_and_navigates`; server `csp_allows_same_origin_workers`.
- [ ] **Step 2: Run** `pnpm test` in `web/` (PowerShell) and `cargo test -p postit-server --test web` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the web gates from `web/CLAUDE.md` and the server test — PASS. Manually: `cargo dev` + `pnpm dev`, enable browser notifications on the profile page, trigger a mock failure through a test-only route or `psql` insert plus `Notifier` call, see the browser notification (record what was done in the commit body).
- [ ] **Step 5: Commit** `feat(web): add browser push and notification settings to the profile page`.

### Task 22: Docs, deferred files, and full gates

**Files:**
- Modify: `docs/configuration.md` (`[push]`, the new `[jobs]` keys and schedules, `[publishing]` notes), `docs/deployment.md` (VAPID key generation for qa and production in the G1 checklist; push stays off until then), `docs/security.md` (push payload rule, endpoint sealing, labels in notification payloads), `!ref/plans/01. vision and architecture.md` (`notifications` payload: "IDs, counts, and display labels"; the `user_preferences` data-model line, which names one `notification_channels jsonb` column, becomes the built `notification_switches jsonb`, `comment_digest bool`, and `last_comment_digest_at`), `CLAUDE.md` (project state: D1 built), `backend/crates/data/src/publishing/mod.rs` (job map), `docs/superpowers/plans/2026-10-24-b5-agent-kit-mock-deferred.md` → `git mv` to `…-deferred-done.md` with a resolution note, `docs/superpowers/plans/2026-10-28-b7-oauth-connection-deferred.md` → `…-deferred-done.md`, `docs/superpowers/plans/2026-10-26-b4-media-deferred.md` and `2026-10-26-b6-delegation-deferred.md` (mark the resolved entries; keep names), `docs/superpowers/plans/2026-10-29-d1-publishing-engine-notifications-deferred.md` (only if something truly cannot be fixed in D1)

- [ ] **Step 1: Run**, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3`, `cargo sqlx prepare --check --workspace`, `cargo xtask openapi --check`, `cargo deny check` (the production audit job's command). From `web/`: the `web/CLAUDE.md` gates. From `mobile/`: the format, analyze, and test commands in `CLAUDE.md`. Expected: all pass; fix anything that does not, minor issues included.
- [ ] **Step 2: Update the docs and deferred files** listed above. Each resolved deferred entry names the D1 task that resolved it.
- [ ] **Step 3: Exit-criteria check** against plan 03 D1: queued work durable, safely claimed, observable, retryable, idempotent (Tasks 7–12 tests); no duplicate after a remote success (Review Focus 1); notifications reach in-app, email, and push as configured (Tasks 17–21). Write the result in the final report.
- [ ] **Step 4: Commit** `docs: record D1 in the configuration, deployment, and security docs and close its deferred items`.
