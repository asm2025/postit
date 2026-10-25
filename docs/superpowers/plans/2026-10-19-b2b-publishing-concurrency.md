# B2b: Publishing Concurrency Queries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the publishing engine (D1) and the REST services (D3) the SQL they need, proven under concurrency. That covers job creation, delivery claims, leases, fenced transitions, user actions, the due sweep and unpark, job status, quotas, direct-send batches with the email cap, recurrence ticks, deletion batches, and retention and OAuth purges.

**Architecture:**
- All of this lives in `postit-data::publishing` (submodules `jobs`, `deliveries`, `sweep`, `quota`, `direct`) plus additions to `recurrences`, `retention`, and a new `deletion` module.
- Delivery state changes go through one fenced `apply` that asks core's `transition()` for the next state and writes it with a `dispatch_gen` and expected-state guard.
- Functions that create work return the IDs and run times for the caller's `JobQueue::enqueue_in`. `postit-data` enqueues nothing.

**Tech Stack:** Rust 2024, sqlx 0.9, PostgreSQL 18 (`FOR UPDATE SKIP LOCKED`, `INSERT … ON CONFLICT DO UPDATE … WHERE`), `sha2`, core's delivery state machine.

**Spec:** `docs/superpowers/specs/2026-10-19-b2-domain-schema-design.md` (sections "Publish jobs and deliveries" through "Deletion, retention, and purges"), which argues from `!ref/plans/03. publishing roadmap.md` phase B2 and the Publishing, Delivery states, Quotas, Sending an email campaign, and Recurring posts flows. Read both. This plan runs after `docs/superpowers/plans/2026-10-19-b2a-schema-scope-repositories.md` on the same branch and uses its interfaces.

## Global Constraints

- Every Global Constraint from the B2a plan applies here too: lints, single migration, compile-checked queries with `cargo sqlx prepare --workspace`, scoped repositories, `#[sqlx::test]`, quality gates after every task, commit format, and American English.
- System writes, meaning anything the worker, the sweep, or a cron does, take no `OwnerScope` and leave `updated_by` null. User actions take `&OwnerScope`, require `Capability::Publish`, and apply the account filter to the delivery's account.
- Every write to `deliveries` after a claim is conditioned on `dispatch_gen` and the expected state, and bumps `publish_jobs.version` in the same transaction. The only exception is `record_late_receipt`, which is conditioned on the state alone.
- The state table lives only in `postit_core::transition`. No SQL hard-codes a transition that core does not allow. A test feeds every `(state, event)` pair through `apply` and compares the result with `transition`.
- Batch sweeps and purges use `FOR UPDATE SKIP LOCKED` sub-selects with a `limit` argument (callers pass about 500).
- Concurrency tests use two or more pool connections with transactions held open, and `tokio::spawn` for the side that must block. They never sleep to order events: they use `tokio::sync::Barrier` or `oneshot`.
- The cutoff times are computed in Rust and bound as `timestamptz`, in the existing `retention.rs` style.

## Review Focus

1. **A worker whose lease expired, which then reports success.** Its fenced write is `Stale`. `record_late_receipt` turns the swept `outcome_unknown` delivery into `published` with the receipt, and a second late receipt changes nothing. Task 2 tests this.
2. **A cap of zero** (unlimited) **or a cap already used up at the start of a campaign.** Zero claims full batches and writes no usage row. A used-up cap claims nothing and reports `CapReached`, leaving every row queued. Task 6 tests this.
3. **Cancelling a job while one of its deliveries is in flight.** Pending deliveries become `cancelled`. The in-flight one only gets `cancel_requested_at`, and the job stays non-terminal until the worker finishes. Task 3 tests this.
4. **A parked delivery whose cause cleared long ago** (due time older than `park_grace`). It stays parked, is returned once in the needs-owner list, and is not returned again on the next sweep. Task 4 tests this.
5. **The same contact in two selected lists, plus one suppressed contact.** It gets one direct-send row, the suppressed contact gets none, and the audience count matches. Task 6 tests this.

---

### Task 1: Job creation and the delivery idempotency key

**Files:**
- Create: `backend/crates/data/src/publishing/mod.rs`, `backend/crates/data/src/publishing/jobs.rs`
- Test: `backend/crates/data/tests/publishing_jobs.rs`

**Interfaces:**
- Consumes: B2a `LockedPost`, `OwnerScope`, `translate`, core `PublishSnapshot`, `JobStatus`.
- Produces:
  - `pub fn delivery_idempotency_key(job: PublishJobId, account: AccountId) -> [u8; 32]`: SHA-256 of the job's 16 UUID bytes followed by the account's 16.
  - `pub enum Operation { Publish, Comment, Direct }` with `as_str`.
  - `pub struct NewJob { pub id: PublishJobId, pub snapshot: PublishSnapshot, pub targets: Vec<NewDelivery>, pub scheduled_at: DateTime<Utc>, pub native_schedule: bool, pub dry_run: bool, pub recurrence_id: Option<RecurrenceId> }`
  - `pub struct NewDelivery { pub id: DeliveryId, pub account: AccountId, pub operation: Operation }`
  - `pub struct CreatedJob { pub job: PublishJobId, pub deliveries: Vec<Dispatch> }`
  - `pub struct Dispatch { pub delivery: DeliveryId, pub dispatch_gen: i32, pub run_at: DateTime<Utc> }`
  - `JobRepo::create(conn, scope, &LockedPost, NewJob) -> Result<CreatedJob, DataError>`. It requires `Publish` for a live job and `Edit` for a dry run. Every target must pass `scope.allows_account`, else `AccountNotInScope`. `post_version` comes from the locked post. The status starts as `scheduled`, or as `publishing` when `scheduled_at <= now()`. A race on the one-live-job index surfaces as `JobActive`.
  - `JobRepo::get(conn, scope, PublishJobId) -> PublishJob` and `JobRepo::list(conn, scope, Pagination, JobFilter { post_id, status })`. A limited delegate sees a job only when at least one of its deliveries is in the limit, and sees only those deliveries.
  - `JobRepo::snapshot(conn, PublishJobId) -> PublishSnapshot` is a system read.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn idempotency_key_is_stable_and_order_sensitive() // key(j,a) == key(j,a); key(j,a) != key(a_as_job, j_as_account)
#[sqlx::test] async fn create_inserts_job_and_pending_deliveries() // 2 targets -> 2 dispatches with gen 0 and run_at == scheduled_at; deliveries.state pending; created_by == actor
#[sqlx::test] async fn duplicate_delivery_rejected_by_idempotency_key() // raw insert of a second delivery with same (job, account) -> unique violation
#[sqlx::test] async fn second_live_job_is_refused() // create live job, commit; lock() again -> Err(PostLocked)
#[sqlx::test] async fn one_live_job_index_maps_to_job_active() // bypass the lock: raw insert of a second non-terminal live job for the post, passed through translate -> JobActive
#[sqlx::test] async fn dry_run_not_covered_by_one_live_job_index() // raw: live job 'publishing' + dry-run job 'publishing' for same post both insert
#[sqlx::test] async fn republish_same_version_after_terminal() // job1 set status 'published' (raw); create job2 same post_version -> Ok
#[sqlx::test] async fn snapshot_unchanged_by_later_edit() // create job; mark it terminal (raw); update_content; snapshot(job) == original snapshot (serde equality)
#[sqlx::test] async fn edit_delegate_cannot_create_live_job() // -> InsufficientAccess; dry run -> Ok
#[sqlx::test] async fn limited_delegate_cannot_target_hidden_account() // -> AccountNotInScope
#[sqlx::test] async fn edit_racing_job_creation() // tx1: lock + create job, hold; tx2 (spawned): lock(Some(v)) blocks; commit tx1 -> tx2 gets PostLocked
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Store `content_snapshot` with `serde_json::to_value`. No method updates that column.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): create publish jobs with snapshot and idempotent deliveries`.

---

### Task 2: Claim, lease, fenced transitions, late receipts, attempts, and job status

**Files:**
- Create: `backend/crates/data/src/publishing/deliveries.rs`
- Test: `backend/crates/data/tests/publishing_deliveries.rs`

**Interfaces:**
- Produces:
  - `DeliveryRepo::claim(conn, DeliveryId, dispatch_gen: i32, now, lease: Duration) -> Result<Option<ClaimedDelivery>, DataError>`, written as the plan's single `UPDATE … RETURNING`. `ClaimedDelivery` carries:
    - the IDs: `id`, `job_id`, `post_id`, `owner_id`, `account_id`
    - the job flags: `dry_run`, `native_schedule`, `operation`
    - the retry state: `dispatch_gen`, `attempt_count`, `container_ref`
    - `cancel_requested: bool`
  - `renew_lease(conn, DeliveryId, dispatch_gen, now, lease) -> Result<bool, DataError>`, for state `claimed` or `in_flight`.
  - `pub struct Fence { pub id: DeliveryId, pub dispatch_gen: i32, pub expected: DeliveryState }`
  - `#[derive(Default)] pub struct TransitionData`, with these optional fields: `remote_id`, `remote_url`, `remote_publish_at`, `last_error: Option<Value>`, `next_attempt_at`, `count_attempt: bool`, `direct_counts: Option<Value>`.
  - `pub enum Applied { Done { state: DeliveryState, redispatch: Option<Dispatch> }, Stale }`
  - `DeliveryRepo::apply(conn, Fence, DeliveryEvent, TransitionData, now) -> Result<Applied, DataError>`:
    - It calls `postit_core::transition(fence.expected, event)`, and maps a `TransitionError` to `DataError::Conflict("transition_not_allowed")`.
    - It writes the new state and `parked_reason` with the `WHERE id AND dispatch_gen AND state (AND parked_reason)` guard.
    - When the new state is `Pending`, it increments `dispatch_gen`, sets `next_attempt_at` (from the data, else `now`), clears the lease, and returns `redispatch`.
    - It bumps `publish_jobs.version`, then calls `refresh_job_status`. All of this happens in the caller's transaction.
  - `set_container_ref(conn, Fence, Value) -> Result<bool, DataError>`, fenced, with no state change.
  - `record_late_receipt(conn, DeliveryId, Receipt { remote_id, remote_url }, now) -> Result<bool, DataError>`. It applies only from `outcome_unknown` with `remote_id IS NULL`.
  - `refresh_job_status(conn, PublishJobId, now) -> Result<JobStatus, DataError>`:
    - It builds `DeliveryView`s, with `awaiting_native_publish` = `state = 'processing' AND remote_publish_at > now`, and writes `publish_jobs.status`.
    - When the job is not a dry run and its `post_version` equals `posts.version`, it sets `posts.status` to the matching post status. `scheduled` maps to `scheduled`, `publishing` to `publishing`, and the terminal statuses map one to one.
    - When the job is not a dry run and the versions differ, it leaves the post as `draft`.
  - `AttemptRepo::start(conn, DeliveryAttemptId, DeliveryId, now)` and `AttemptRepo::finish(conn, DeliveryAttemptId, outcome: &str, error: Option<Value>, request_summary: Option<Value>, now)`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn claim_once() // two connections claim same (id, gen 0) concurrently -> exactly one Some
#[sqlx::test] async fn stale_gen_claim_finds_nothing() // apply RetryableFailure -> gen 1; claim(gen 0) None; claim(gen 1) Some after next_attempt_at
#[sqlx::test] async fn future_delivery_not_claimable_yet()
#[sqlx::test] async fn cancelled_job_not_claimable() // job cancelled_at set -> None
#[sqlx::test] async fn apply_matches_core_table() // for every DeliveryState x DeliveryEvent: set row to state via raw SQL, apply; Ok(Done{state}) iff transition(..) == Ok(state), else Conflict; no row changed on Conflict
#[sqlx::test] async fn fenced_write_after_takeover_is_stale() // claim gen0, StartRemote; simulate sweep: raw lease_expires_at in past + apply(LeaseExpired) from InFlight (gen0) -> OutcomeUnknown; original worker apply(Published, Fence{gen0, InFlight}) -> Stale
#[sqlx::test] async fn late_receipt_publishes_once() // after the takeover above: record_late_receipt -> true, state published, remote_id set; second call -> false
#[sqlx::test] async fn retry_redispatch_increments_gen_and_sets_time() // apply RetryableFailure with next_attempt_at t -> redispatch{gen 1, run_at t}
#[sqlx::test] async fn every_apply_bumps_job_version()
#[sqlx::test] async fn job_and_post_status_follow_deliveries() // 2 deliveries: one published, one failed -> job partial, post partial; edit post (bump) then refresh -> post draft
#[sqlx::test] async fn dry_run_job_never_touches_post_status()
#[sqlx::test] async fn renew_lease_only_for_current_gen()
#[sqlx::test] async fn container_ref_is_fenced()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** `apply` must keep its SQL shape fixed for every event, so it stays one compile-checked query with nullable binds (`COALESCE($x, column)` for data fields). Build no dynamic SQL.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add delivery claims, leases, fenced transitions, and job status`.

---

### Task 3: User actions on jobs and deliveries

**Files:**
- Modify: `backend/crates/data/src/publishing/deliveries.rs`, `backend/crates/data/src/publishing/jobs.rs`, `backend/crates/data/src/media.rs` (snapshot rule for `delete`)
- Test: `backend/crates/data/tests/publishing_actions.rs`

**Interfaces:**
- Produces (each takes `scope` with `Publish`, and each delivery's account must be in the limit):
  - `DeliveryRepo::get(conn, scope, DeliveryId)` and `DeliveryRepo::list_for_job(conn, scope, PublishJobId)`. Both are filtered, and both need only `View`.
  - `DeliveryRepo::cancel(conn, scope, DeliveryId, now) -> CancelOutcome`. `CancelOutcome` is one of:
    - `Cancelled`, for pending or parked (bumps the generation)
    - `Requested`, for in-flight (sets `cancel_requested_at`)
    - `NeedsAgent { remote_publish_at }`, for a processing native-scheduled upload, which D1 deletes through the agent
    - `NotCancellable`, for any other state
  - `JobRepo::cancel(conn, scope, PublishJobId, now) -> Vec<(DeliveryId, CancelOutcome)>`. It applies the whole-object rule: every non-terminal delivery must be in the limit, else `AccountNotInScope`. It sets `publish_jobs.cancelled_at`.
  - `DeliveryRepo::cancel_for_account(conn, AccountId, now) -> Vec<(DeliveryId, CancelOutcome)>` is a system call, used by disconnect.
  - `DeliveryRepo::retry(conn, scope, DeliveryId, acknowledged: bool, now) -> Dispatch`:
    - It uses `UserRetry` through `apply`, with `attempt_count` reset to 0.
    - It returns `JobActive` when the job is terminal and the post has another non-terminal live job.
    - `AcknowledgementRequired` maps to `DataError::Conflict("acknowledgement_required")`.
  - `DeliveryRepo::resolve(conn, scope, DeliveryId, ResolvedOutcome, now)`.
  - `JobRepo::reschedule_pending(conn, scope, PublishJobId, at: DateTime<Utc>, now) -> Result<Vec<Dispatch>, DataError>`. It succeeds only when every non-terminal delivery is `pending`, or is `processing` with a `remote_publish_at` (the native-scheduled ones are returned as `NeedsAgent` for D1 to move). Otherwise it returns `Conflict("not_reschedulable")` and changes nothing.
  - `MediaRepo::delete` now also returns `Conflict("media_in_use")` while any non-terminal job's `content_snapshot` references the asset ID. Use a jsonb containment or path query over the snapshot's media items.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn cancel_job_with_in_flight_delivery() // d1 pending, d2 in_flight -> d1 Cancelled, d2 Requested; job status publishing
#[sqlx::test] async fn limited_delegate_cannot_cancel_job_with_hidden_delivery() // -> AccountNotInScope; can cancel its own delivery individually
#[sqlx::test] async fn retry_from_outcome_unknown_needs_ack()
#[sqlx::test] async fn retry_resets_attempts_and_redispatches()
#[sqlx::test] async fn retry_refused_while_another_live_job_active() // -> JobActive
#[sqlx::test] async fn reschedule_refused_when_one_in_flight() // -> not_reschedulable, nothing changed (times and gens identical)
#[sqlx::test] async fn reschedule_moves_pending_with_new_gen()
#[sqlx::test] async fn resolve_dead_letter_to_published()
#[sqlx::test] async fn media_referenced_by_active_snapshot_in_use() // post no longer uses media, active job snapshot does -> media_in_use; after job terminal -> delete ok
#[sqlx::test] async fn cancel_for_account_cancels_pending_and_parked_only()
#[sqlx::test] async fn view_delegate_cannot_act() // InsufficientAccess for cancel/retry/resolve/reschedule
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** on top of `apply`.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add cancel, retry, resolve, and reschedule for publish jobs`.

---

### Task 4: Due sweep and unpark

**Files:**
- Create: `backend/crates/data/src/publishing/sweep.rs`
- Test: `backend/crates/data/tests/publishing_sweep.rs`

**Interfaces:**
- Produces (system calls; each takes `now` and a `limit`):
  - `Sweep::overdue_pending(conn, now, sweep_grace, limit) -> Vec<Dispatch>`. This bumps the generation.
  - `Sweep::expired_claims(conn, now, limit) -> Vec<Dispatch>`. A `claimed` delivery with an expired lease goes back to `pending`.
  - `Sweep::expired_in_flight(conn, now, limit) -> Vec<DeliveryId>`. Each goes to `outcome_unknown`, for D1 to enqueue `reconcile_delivery`.
  - `Sweep::orphaned_direct(conn, now, limit) -> Vec<DeliveryId>`: `processing` direct deliveries that have `queued` rows and no row with a live lease.
  - `pub struct UnparkInput<'a> { pub now: DateTime<Utc>, pub park_grace: Duration, pub compiled_platforms: &'a [PlatformId], pub limit: i64 }`
  - `pub struct UnparkResult { pub resumed: Vec<Dispatch>, pub needs_owner: Vec<(UserId, DeliveryId)> }`
  - `Sweep::unpark(conn, UnparkInput) -> UnparkResult`. It applies the per-reason cleared conditions from the spec:
    - `owner_disabled` clears when the owner is `active`.
    - `account_needs_reconnect` clears when the account is `active`.
    - `platform_unavailable` clears when the platform is in `compiled_platforms`.
    - `platform_disabled` clears when `platforms.enabled` is true.
    - `live_publish_disabled` clears when `app_settings` `publishing.live` is true, for live deliveries.
    - `quota_exceeded` clears when `next_attempt_at <= now`.
    - `outcome_unresolvable` never clears.

    A cleared delivery whose due time (`next_attempt_at`) is within `park_grace` goes back to `pending` through `apply(Unpark)`. A cleared delivery older than that, with `parked_notified_at IS NULL`, gets `parked_notified_at = now` and is returned in `needs_owner`.
  - `Sweep::native_scheduled_counts(conn) -> Vec<(PlatformId, i64)>` returns counts only, for the kill-switch dialog and doctor.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn overdue_pending_redispatched_once() // two concurrent sweeps -> union of results has the delivery once
#[sqlx::test] async fn expired_claim_returns_to_pending()
#[sqlx::test] async fn expired_in_flight_becomes_outcome_unknown()
#[sqlx::test] async fn orphaned_direct_detected() // queued rows, all leases expired -> listed; one live lease -> not listed
#[sqlx::test] async fn unpark_each_reason_when_cleared() // table-driven over the six clearable reasons
#[sqlx::test] async fn outcome_unresolvable_never_unparks()
#[sqlx::test] async fn stale_parked_notifies_owner_once() // due 1h ago, cause cleared -> needs_owner once; second sweep -> not returned; state still parked
#[sqlx::test] async fn unpark_respects_platform_not_compiled() // compiled_platforms excludes it -> stays parked
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Each sweep is one `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED LIMIT $n) RETURNING`. `unpark` may select candidates first, then call `apply` for each in the same transaction.
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add the due sweep and unpark queries`.

---

### Task 5: Quotas and the email daily cap

**Files:**
- Create: `backend/crates/data/src/publishing/quota.rs`
- Test: `backend/crates/data/tests/publishing_quota.rs`

**Interfaces:**
- Produces:
  - `pub struct AccountQuotaKey { pub owner: UserId, pub account: AccountId, pub platform: PlatformId, pub bucket: String, pub period_start: NaiveDate }`
  - `pub struct AppQuotaKey { pub platform: PlatformId, pub bucket: String, pub period_start: NaiveDate }`
  - `QuotaRepo::reserve(conn, &AccountQuotaKey, cost: i64, limit: i64) -> Result<bool, DataError>` and `QuotaRepo::release(conn, &AccountQuotaKey, cost: i64)`. Both are system calls.
  - `QuotaRepo::reserve_up_to(conn, &AccountQuotaKey, want: i64, limit: i64) -> Result<i64, DataError>`. `limit == 0` returns `want` and writes nothing.
  - `PlatformAppQuotaRepo::reserve`, `release`, and `usage(conn) -> Vec<(PlatformId, String, NaiveDate, i64)>`. The usage rows carry no user reference.
  - `pub const EMAIL_CAP_BUCKET: &str = "daily_cap"`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn reserve_within_limit_then_refuse() // limit 3, cost 2: true, false (2+2>3), release 2, true
#[sqlx::test] async fn concurrent_reservations_never_exceed_limit() // 20 tasks on 20 pool connections, limit 7, cost 1 -> exactly 7 true; units_used == 7
#[sqlx::test] async fn reserve_up_to_grants_remainder() // limit 10, used 8, want 5 -> 2; then 0
#[sqlx::test] async fn reserve_up_to_unlimited_writes_nothing() // limit 0 -> want, no row
#[sqlx::test] async fn periods_are_separate_rows()
#[sqlx::test] async fn release_never_goes_negative()
#[sqlx::test] async fn app_quota_concurrent_never_exceeds()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** each reservation as one `INSERT … ON CONFLICT (…) DO UPDATE SET units_used = … WHERE … RETURNING`. Handle the first insert when `cost > limit` by not inserting at all (a guard in the `VALUES` select).
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add atomic quota and email-cap reservations`.

---

### Task 6: Direct sends

**Files:**
- Create: `backend/crates/data/src/publishing/direct.rs`
- Test: `backend/crates/data/tests/publishing_direct.rs`

**Interfaces:**
- Consumes: Task 5 `reserve_up_to`/`release`, B2a `email_hash`, Task 2 `apply`.
- Produces:
  - `pub struct Audience { pub lists: Vec<ContactListId>, pub tags: Vec<String> }`. All tags must match. An empty `tags` means no filter.
  - `DirectRepo::materialize(conn, &ClaimedDelivery, &Audience) -> Result<i64, DataError>`. This is the one `INSERT … SELECT DISTINCT` described in the spec, and it skips anything not `subscribed` and anything suppressed. It writes `direct_counts.audience` and `direct_counts.queued`. It is idempotent: a second call inserts nothing and returns the stored audience.
  - `pub enum BatchClaim { Rows(Vec<DirectSendRow>), CapReached, Empty }`. `DirectSendRow { id, contact_id, email_hash }` holds no address. C2 loads addresses through the contacts repository.
  - `DirectRepo::claim_batch(conn, DeliveryId, batch: i32, size: i64, now, lease, cap: Option<(&AccountQuotaKey, i64)>) -> Result<BatchClaim, DataError>`. In one transaction it reserves up to `size`, then leases that many queued rows with an expired or absent lease, ordered by `id`, `FOR UPDATE SKIP LOCKED`. It releases any reserved units it did not use.
  - `DirectRepo::record(conn, DirectSendId, batch: i32, DirectResult)`, where `DirectResult` is `Sent { provider_message_id }`, `Failed { code }`, `Suppressed`, or `Bounced { code }`. It is conditioned on the row being leased to `batch`. `Bounced` also adds a `bounced` suppression for the row's `email_hash`.
  - `DirectRepo::progress(conn, DeliveryId) -> DirectCounts`, with fields `audience`, `queued`, `sent`, `failed`, `suppressed`, and `bounced`.
  - `DirectRepo::finish_if_done(conn, Fence, now) -> Result<Option<Applied>, DataError>`. When no row is `queued`, it calls `apply(Published)` with the counts in `direct_counts`.
  - `DirectRepo::cancel_queued(conn, DeliveryId) -> i64` marks queued rows `failed` with code `cancelled`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn materialize_dedupes_and_skips_suppressed() // contact c1 in lists L1,L2; c2 suppressed; c3 unsubscribed; c4 bounced -> audience 1
#[sqlx::test] async fn materialize_tag_filter()
#[sqlx::test] async fn materialize_is_idempotent()
#[sqlx::test] async fn two_workers_claim_disjoint_batches() // 120 rows, size 50, two connections concurrently -> disjoint id sets, total 100
#[sqlx::test] async fn cap_reached_mid_campaign_leaves_rows_queued() // cap 30, 120 rows, size 50 -> first claim 30 rows; next CapReached; 90 queued
#[sqlx::test] async fn unlimited_cap_claims_full_batch_without_usage_row()
#[sqlx::test] async fn expired_batch_lease_rows_reclaimable()
#[sqlx::test] async fn record_requires_matching_batch() // record with wrong batch -> no change
#[sqlx::test] async fn bounce_adds_suppression()
#[sqlx::test] async fn finish_when_all_terminal_publishes_with_counts() // includes failures -> published, counts match
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add direct-send audience, batches, and cap-aware claims`.

---

### Task 7: Recurrence tick queries

**Files:**
- Modify: `backend/crates/data/src/recurrences.rs`
- Test: `backend/crates/data/tests/recurrences_tick.rs`

**Interfaces:**
- Produces (system calls):
  - `RecurrenceRepo::due(conn, until: DateTime<Utc>, limit) -> Vec<DueRecurrence>`. `DueRecurrence` holds `id`, `post_id`, `owner_id`, `next_occurrence_at`, `targets`, `rewrite`, and `confirmed_version`. The query selects unpaused rows with `next_occurrence_at <= until` and uses `FOR UPDATE SKIP LOCKED`.
  - `RecurrenceRepo::advance(conn, RecurrenceId, expected_next: DateTime<Utc>, new_next: Option<DateTime<Utc>>) -> Result<bool, DataError>`. `None` sets `paused = true, paused_reason = 'ended'`.
  - `RecurrenceRepo::record_skip(conn, RecurrenceId, at, reason: &str)` keeps the newest 20 entries in `skipped`.
  - `JobRepo::create_for_occurrence(conn, owner: UserId, post_id, NewJob) -> Result<CreatedJob, DataError>`. This is a system variant of `create`. It locks the post `FOR UPDATE`. It returns `PostLocked` when a non-terminal live job exists, so the tick records a skip. `created_by` is the recurrence's `confirmed_by`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn two_ticks_advance_once() // two connections: both call due() then create_for_occurrence + advance in own tx -> exactly one job created; next_occurrence_at advanced once
#[sqlx::test] async fn paused_recurrence_not_due()
#[sqlx::test] async fn advance_none_ends_recurrence()
#[sqlx::test] async fn occurrence_skipped_while_previous_job_active() // -> PostLocked; record_skip appends
#[sqlx::test] async fn skipped_keeps_newest_twenty()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add recurrence tick queries`.

---

### Task 8: Deletion batches, retention purges, and the OAuth purge

**Files:**
- Create: `backend/crates/data/src/deletion.rs`
- Modify: `backend/crates/data/src/retention.rs`, and the `data_retention` job in `backend/crates/server/src/` (call the new purges with `settings.retention`)
- Test: `backend/crates/data/tests/deletion.rs`, `backend/crates/data/tests/retention.rs` (extend)

**Interfaces:**
- Produces:
  - `pub const OWNED_BATCH_TABLES: &[&str] = &["direct_sends", "email_events", "delivery_attempts", "inbox_items", "post_metric_snapshots", "contacts"]`
  - `delete_owned_batch(conn, owner: UserId, table: &str, limit: i64) -> Result<u64, DataError>`. It asserts that `table` is in the constant (the same idea as `assert_identifier`). It deletes `WHERE id IN (SELECT id … WHERE owner_id = $1 LIMIT $2)`.
  - `retention::purge_read_notifications(conn, cutoff)`, `purge_delivery_attempts(conn, cutoff)`, `clear_ai_prompts(conn, cutoff)`, `purge_ai_generations(conn, cutoff)`, `purge_inbox_items(conn, cutoff)`, `purge_direct_sends(conn, cutoff)`, `purge_email_events(conn, cutoff)`, `purge_contact_imports(conn, cutoff)`. Each returns `u64` and is batched internally with a limit of 5000 per statement, looping until zero.
  - `retention::purge_expired_connections(conn, now) -> (u64, u64)` covers `oauth_states` and `pending_connections`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn deleting_owner_cascades_everything_and_spares_others() // seed owner A with one row in every owned table (helper), owner B likewise, A's post edited by delegate D (updated_by = D); batch-delete A's big tables, then DELETE users A -> no row with owner_id A in any table; B intact; delete D -> B's rows updated_by NULL, still present
#[sqlx::test] async fn delete_owned_batch_respects_limit()
#[sqlx::test] async fn each_retention_purge_deletes_only_expired() // table-driven: one old and one fresh row per table
#[sqlx::test] async fn unread_notifications_kept()
#[sqlx::test] async fn ai_prompt_cleared_before_row_deleted()
#[sqlx::test] async fn expired_connections_purged()
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Wire the purges into `postit-server`'s existing `data_retention` handler, and log the counts the way the existing purges do.
- [ ] **Step 4: Run** `cargo test -p postit-data -p postit-server` and `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(data): add owner deletion batches and plan 03 retention purges`.

---

### Task 9: Full gates and handoff notes

- [ ] **Step 1: Run**, from `backend/`:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo check --workspace --all-targets`
  - `cargo test --workspace`
  - `cargo sqlx prepare --check --workspace`
  - `cargo xtask openapi --check`

  Expected: all pass.
- [ ] **Step 2:** Add module-level rustdoc to `publishing/mod.rs` that lists which D1 job calls which function (`publish_delivery`: `claim`, `apply`, `set_container_ref`; `delivery_due_sweep`: `Sweep::*`; `direct_send_batch`: `claim_batch`, `record`, `finish_if_done`; `recurrence_tick`: `due`, `create_for_occurrence`, `advance`). Keep it to a short table.
- [ ] **Step 3: Commit** `docs(data): map publishing queries to their jobs`.
