# B2b Follow-up: Direct-Send Hardening and Concurrency Proofs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the review findings of `docs/superpowers/plans/2026-10-19-b2b-publishing-concurrency.md` that were not fixed with that plan, so D1 builds on a direct-send path that cannot be misused, spin, or strand rows, and the concurrency tests actually force the interleavings they claim to cover.

**Architecture:** Everything stays in `postit-data::publishing` (`direct`, `deliveries`, `sweep`) and its tests, plus one ID newtype in `postit-core`. No new module, no migration file: the single-migration policy still holds, so the one schema change (Task 1) edits `0001_init.sql`.

**Tech Stack:** Rust 2024, sqlx 0.9, PostgreSQL 18, `tokio::time::timeout`.

**Spec:** `docs/superpowers/specs/2026-10-19-b2-domain-schema-design.md` (sections "Publish jobs and deliveries" through "Quotas" and "Sending an email campaign"), arguing from `!ref/plans/03. publishing roadmap.md` phase B2, D1, and "Sending an email campaign". This plan runs after B2b and before D1.

## Where these items come from

The B2b whole-branch review found four Important issues and a list of minors. The Important ones and the cheap minors were fixed in B2b (commits `d696c03` and `858ca81`). A review of the first draft of this follow-up then found that its cap handling contradicted plan 03, that its batch-number lock defeated the concurrency it was meant to prove, and that cancelling a direct delivery was broken. This version carries all of it.

## Decisions

- **A cap-blocked campaign is not parked.** Plan 03 ("Sending an email campaign", step 5): "the remaining rows wait for the next day and continue automatically … This is not a parking state". The delivery stays `processing` and carries the time it continues in `next_attempt_at`. No core transition changes.
- **Batch IDs are UUIDs allocated by the data layer**, not numbers chosen by the caller and not a counter on the delivery row. A UUID needs no lock, so two workers still claim batches of one delivery in parallel, and it cannot repeat across workers, restarts, or console retries of an apalis job.
- **A late result wins over a cancel.** Cancelling a running campaign marks every queued row `failed`/`cancelled`, including rows a live batch is sending. When that batch records its real result, `record` replaces the cancel with it, so per-recipient results stay exact (plan 03 step 6). Chosen by the maintainer on 2026-10-23.
- **Lock order for direct deliveries:** delivery row, then job row, then `direct_sends` rows. `claim_batch` takes the quota row, then a share lock on the delivery, then rows; it never takes the job. No path takes `direct_sends` and then the job.
- **`resume_at` is computed by D1** from the same period definition it uses to build the `AccountQuotaKey` (spec, "No time-zone arithmetic"). The data layer stores and compares it.

## Global Constraints

- Every Global Constraint of the B2b plan applies: lints, single migration, compile-checked queries with `cargo sqlx prepare --workspace`, `#[sqlx::test]`, quality gates after every task, commit format, American English.
- Concurrency tests never sleep to order events. They hold the contended rows in an open transaction and use one of:
  - **two transactions in one task**, for statements with `SKIP LOCKED`: run the statement in A, then in B while A is open (the pattern of `two_ticks_advance_once` in `recurrences_tick.rs`);
  - **a spawned task plus `wait_for_blocked_statement`**, for statements that wait on a row lock: commit A only once B is seen blocked in `pg_stat_activity`.
- Every step that could block when the code under test loses its lock runs inside `tokio::time::timeout` (5 seconds), so a lost lock fails the test instead of hanging CI.
- Repository functions call `conn.begin()` internally. Inside a test's open transaction that is a savepoint, so the locks the function takes stay held until the test commits. Tests rely on this; it is how they hold rows open around a repository call.
- A signature change in `DirectRepo`, `DeliveryRepo`, or `Sweep` updates every caller in this repository in the same commit.

## Review Focus

1. **A campaign that hits the daily cap.** It stays `processing`, shows when it continues, and resumes at that time. No sweep lists it while it waits, so nothing enqueues a no-op batch on every run.
2. **Two workers on one direct delivery after a lease expiry.** A worker that lost its batch cannot record onto rows another worker now holds. A slow batch can keep its rows by renewing the lease. A batch job holding a stale generation claims nothing.
3. **Cancelling a direct delivery.** Every cancel, from any state, stops its queued rows in the same transaction and stores the final counts. A batch in flight at that moment still records what really happened.
4. **A crashed batch.** The units it reserved stay counted, and a reclaimed batch reserves again. This is the intended, safe direction; it is documented and pinned by a test.
5. **The concurrency tests named in Task 5.** Each fails if the code under test loses its lock or fence, because the test holds the contended rows open while the second caller runs.

---

### Task 1: Batch IDs from the data layer, a fenced claim, and lease renewal

**Files:**
- Modify: `backend/crates/core/src/ids.rs`, `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/publishing/direct.rs`
- Modify: `docs/superpowers/specs/2026-10-19-b2-domain-schema-design.md` (the "Batch claim" bullet), `!ref/plans/03. publishing roadmap.md` (the `direct_send_batch` row of the job table)
- Test: `backend/crates/data/tests/publishing_direct.rs`

**Why:** `DirectRepo::record` is conditioned on the batch the row was leased to, but the caller chooses that number today. If D1 reuses a number across workers (a per-delivery counter that resets after a restart, or a console retry of the same apalis job), a stale worker's `record` lands on rows another worker now holds. A lock on the delivery row would make the number safe but would serialize every claim of a campaign, which is what `FOR UPDATE SKIP LOCKED` exists to avoid. Separately, `claim_batch` takes a bare `DeliveryId`, so a batch job for a delivery that was cancelled, swept, or re-dispatched can still lease rows; and nothing lets a slow batch extend its lease, so a batch that sends slower than the lease is reclaimed and sent twice.

**Interfaces:**
- Adds: `postit_core::DirectBatchId` (`id_newtype!`, "Identifier of one direct-send batch.").
- Schema: `direct_sends.batch` becomes `UUID` (nullable, as now).
- Changes: `DirectRepo::claim_batch(conn, fence: Fence, size, now, lease, cap) -> Result<BatchClaim, DataError>`. `fence.expected` is `DeliveryState::Processing`.
  - First, `SELECT 1 FROM deliveries WHERE id = $1 AND dispatch_gen = $2 AND state = 'processing' FOR SHARE`. No row: return `BatchClaim::Stale` without reserving cap units. A share lock does not conflict with another claim's share lock, so concurrent claims still run in parallel; it does make a cancel or a transition of the delivery wait until the claim commits, so the fence holds for the whole claim.
  - Then the cap reservation and the row lease as today. The batch ID is generated once per claim (`WITH b AS (SELECT uuidv7() AS id)`), not per row.
  - `BatchClaim::Rows` becomes `Rows { batch: DirectBatchId, rows: Vec<DirectSendRow> }`. Adds `BatchClaim::Stale`.
- Changes: `DirectRepo::record(conn, id, batch: DirectBatchId, result, now)`.
- Adds: `DirectRepo::renew_batch(conn, delivery: DeliveryId, batch: DirectBatchId, now, lease) -> Result<u64, DataError>`: `UPDATE direct_sends SET lease_expires_at = $now + $lease, updated_at = $now WHERE delivery_id = $1 AND batch = $2 AND state = 'queued' AND lease_expires_at IS NOT NULL`, returning how many rows it renewed. `0` means the batch lost all its rows; the worker stops.
- The `direct` test fixture moves its delivery from `claimed` through `in_flight` to `processing` (`StartRemote`, then `Accepted`) and keeps the `Fence`, because every claim is now fenced.

- [ ] **Step 1: Write failing tests.** Change the existing calls to the new signatures first; they then fail to compile, which is the red state. Rewrite `expired_batch_lease_rows_reclaimable` and `record_requires_matching_batch` for `DirectBatchId` rather than adding near-duplicates.

```rust
#[sqlx::test] async fn each_claim_gets_a_new_batch_id() // two claims, and a claim after a lease expiry, never share an ID
#[sqlx::test] async fn a_worker_that_lost_its_batch_cannot_record() // claim (batch A), expire the lease, second claim takes the rows (batch B), record with A returns false and changes nothing
#[sqlx::test] async fn an_expired_but_unclaimed_batch_still_records() // claim, expire the lease, nobody reclaims, record with the batch succeeds: the mail went out
#[sqlx::test] async fn claim_with_a_stale_fence_leases_nothing() // dispatch_gen bumped -> Stale, no row leased, no cap unit used; same for a delivery no longer processing
#[sqlx::test] async fn renew_keeps_a_slow_batch() // claim, renew before expiry, a second claim at the old expiry time finds nothing; renew by a batch that lost its rows returns 0
```

- [ ] **Step 2: Run** `cargo test -p postit-data --test publishing_direct`. Expected: FAIL.
- [ ] **Step 3: Implement.** Update the `claim_batch` and `record` rustdoc: the batch ID is the data layer's, the claim is fenced, and a batch that sends for longer than its lease calls `renew_batch`.
- [ ] **Step 4: Update the documents.** In the spec's "Batch claim" bullet, replace `batch_no` with the fence and the returned batch ID. In plan 03's job table, the `direct_send_batch` arguments become "delivery, dispatch generation" (the job no longer carries a batch number).
- [ ] **Step 5: Run** the tests, then `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 6: Commit** `fix(data): allocate direct-send batch IDs in the data layer and fence batch claims`.

---

### Task 2: A cap-blocked campaign waits in `processing`, and the orphan sweep claims what it lists

**Files:**
- Modify: `backend/crates/data/src/publishing/direct.rs`, `backend/crates/data/src/publishing/sweep.rs`
- Test: `backend/crates/data/tests/publishing_direct.rs`, `backend/crates/data/tests/publishing_sweep.rs`

**Why:** A `processing` direct delivery that stopped at `CapReached` has queued rows and no live lease, so `Sweep::orphaned_direct` lists it on every run until the cap resets, and D1 would enqueue a no-op batch each time. The same query lists a delivery that reached `processing` a moment ago and has no lease yet. It is also a plain `SELECT` with no lock: two overlapping sweeps both list the same delivery, which breaks the module's own promise ("picks its rows with `FOR UPDATE SKIP LOCKED`") and spec line "All sweep updates use `FOR UPDATE SKIP LOCKED` sub-selects", and a sweep that runs again before the enqueued job starts lists it again.

**Interfaces:**
- Adds: `DirectRepo::wait_for_cap(conn, fence: Fence, resume_at: DateTime<Utc>, now) -> Result<bool, DataError>`. A fenced write that changes no state (like `DeliveryRepo::set_container_ref`): `UPDATE deliveries SET next_attempt_at = $resume_at, updated_at = $now WHERE id AND dispatch_gen AND state = 'processing'`, then `touch_job`. Returns `false` when the fence no longer matches. `next_attempt_at` is what the delivery shows as "continues at", so the UI can say "1,200 of 3,000 sent; continues tomorrow".
- Changes: `Sweep::orphaned_direct(conn, now, grace: Duration, limit) -> Result<Vec<DeliveryId>, DataError>` becomes an `UPDATE deliveries SET updated_at = $now WHERE id IN (SELECT … FOR UPDATE OF d SKIP LOCKED) RETURNING id`, then touches each job, as `overdue_pending` does. With `cutoff = now - grace`, the sub-select adds `d.updated_at <= cutoff AND (d.next_attempt_at IS NULL OR d.next_attempt_at <= cutoff)` to today's conditions. So:
  - a delivery that just reached `processing` is skipped until the grace passes;
  - a delivery waiting for its cap is skipped until its resume time plus the grace (the job D1 enqueued for `resume_at` gets the first chance);
  - a listed delivery is stamped, so it is listed at most once per grace, however often the sweep runs.
- D1 passes `publishing.sweep_grace` as `grace`.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn orphaned_direct_skips_a_delivery_inside_the_grace() // processing just now, no lease -> not listed; aged past grace -> listed
#[sqlx::test] async fn orphaned_direct_lists_once_per_grace() // listed, run again at the same now -> empty; run again after another grace -> listed
#[sqlx::test] async fn two_sweeps_never_list_one_orphan_twice() // two transactions in one task: A lists it while open, B returns empty without blocking (timeout)
#[sqlx::test] async fn cap_reached_campaign_waits_and_resumes() // CapReached, wait_for_cap(resume_at): state stays processing, next_attempt_at = resume_at, not listed before resume_at + grace, listed after; queued rows intact
#[sqlx::test] async fn wait_for_cap_is_fenced() // stale dispatch_gen -> false, next_attempt_at unchanged
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Update the `orphaned_direct` rustdoc with the grace, the stamp, and the cap wait.
- [ ] **Step 4: Run** the tests, then `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `fix(data): keep cap-blocked direct deliveries out of the orphan sweep and lock what it lists`.

---

### Task 3: Cancelling a direct delivery stops its rows

**Files:**
- Modify: `backend/crates/data/src/publishing/deliveries.rs` (`apply`, `cancel_loaded`), `backend/crates/data/src/publishing/direct.rs` (`record`, `cancel_queued`)
- Test: `backend/crates/data/tests/publishing_direct.rs`, `backend/crates/data/tests/publishing_actions.rs`

**Why:** Plan 03's transition list has "`Processing` → `Cancelled` (… a direct delivery cancelled with its queued rows)", but `cancel_loaded` returns `NotCancellable` for a `processing` delivery without `remote_publish_at`, which is every direct delivery. So a running campaign cannot be cancelled at all, and with Task 2 a campaign can wait days on its cap. A direct delivery cancelled from `pending`, `claimed`, or `parked` after its audience was materialized keeps its rows `queued` forever: nothing calls `cancel_queued`, `purge_direct_sends` keeps `queued` rows, and `progress` keeps counting them.

**Interfaces:**
- `cancel_loaded`: a `processing` delivery whose operation is `direct` is cancelled through `apply(Cancel)` like `pending` and `parked`, and returns `CancelOutcome::Cancelled`. A natively scheduled `processing` delivery keeps `NeedsAgent`; any other `processing` delivery keeps `NotCancellable`.
- `DeliveryRepo::apply`: when the new state is `Cancelled` and the delivery's operation is `direct`, it calls `DirectRepo::cancel_queued` after `touch_job`, in the same transaction, then stores the final counts from `progress` in `direct_counts`, as `finish_if_done` does for `Published`. Doing it in `apply` covers every cancel path at once: user cancel, job cancel, `cancel_for_account`, and the cancelled-job rewrite at the top of `apply`. The `RETURNING` clause adds `operation`.
- `DirectRepo::record`: the condition becomes `batch = $2 AND ((state = 'queued' AND lease_expires_at IS NOT NULL) OR (state = 'failed' AND error_code = 'cancelled'))`. A row cancelled while its batch was sending takes the real result; a row that was never leased has no batch and stays cancelled. When it replaces a cancel, `record` also refreshes the delivery's stored `direct_counts` in the same transaction. The rows are already unlocked by the committed cancel, so this takes `direct_sends` then `deliveries` only after the cancel is done, and cannot deadlock with it.
- `cancel_queued` stays public for D1 but its rustdoc says `apply` already calls it on every cancel.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn cancelling_a_running_campaign_stops_its_rows() // processing direct delivery, DeliveryRepo::cancel -> Cancelled, every queued row failed/cancelled, direct_counts stored
#[sqlx::test] async fn cancelling_a_job_stops_its_campaign_rows() // JobRepo::cancel with a processing direct delivery and a pending social one: both cancelled, rows stopped
#[sqlx::test] async fn cancel_before_processing_stops_materialized_rows() // materialized while claimed, lease expired to pending, cancel -> no row left queued
#[sqlx::test] async fn a_batch_in_flight_at_cancel_records_its_real_result() // claim batch, cancel, record Sent -> true, row sent, stored counts updated; record again -> false
#[sqlx::test] async fn a_bounce_after_cancel_still_suppresses() // claim, cancel, record Bounced -> row bounced, address suppressed
#[sqlx::test] async fn an_unleased_cancelled_row_cannot_be_recorded() // row never leased, cancel, record with any batch -> false
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Update the rustdoc of `cancel_loaded`, `CancelOutcome::NotCancellable`, `record`, and `cancel_queued`.
- [ ] **Step 4: Run** `cargo test -p postit-data`, then `cargo sqlx prepare --workspace`. Expected: PASS. `publishing_review.rs::cancel_does_not_deadlock_with_a_worker_write` must still pass.
- [ ] **Step 5: Commit** `fix(data): cancel direct deliveries with their queued rows and keep late batch results`.

---

### Task 4: Pin and document the crashed-batch cap behavior

**Files:**
- Modify: `backend/crates/data/src/publishing/direct.rs` (module and `claim_batch` rustdoc)
- Test: `backend/crates/data/tests/publishing_direct.rs`

**Why:** A batch whose worker died after reserving daily-cap units keeps those units, and reclaiming its rows reserves again. This is deliberate: some of that mail may have been sent but not recorded. The `claim_batch` rustdoc states the rule but not what the user sees, and no test pins it.

- [ ] **Step 1: Write the test.** Daily-cap usage has no reader in `QuotaRepo` (`usage` belongs to `PlatformAppQuotaRepo`), so the test reads `platform_quota_usage` with raw SQL.

```rust
#[sqlx::test] async fn a_reclaimed_batch_counts_against_the_cap_again()
// queue 10, cap 10: claim 5 (used 5), expire the lease, claim again -> the same 5 rows (lowest IDs), used 10;
// the next claim -> CapReached with 5 rows never sent
```

- [ ] **Step 2: Run** it. Expected: PASS already, since it pins existing behavior. If it fails, the behavior is not what the B2b ledger says; stop and report.
- [ ] **Step 3:** Extend the rustdoc: what is over-counted, why that direction is safe, and that after a crash a user may see their day's cap used up with recipients left for tomorrow. No user-facing page describes the cap yet; add one sentence to the spec's "Email daily cap" bullet.
- [ ] **Step 4: Commit** `docs(data): document how a crashed direct-send batch counts against the cap`.

---

### Task 5: Concurrency tests that force the interleaving

**Files:**
- Modify: `backend/crates/data/tests/common/mod.rs`, `backend/crates/data/tests/publishing_review.rs`, `backend/crates/data/tests/publishing_deliveries.rs`, `backend/crates/data/tests/publishing_sweep.rs`, `backend/crates/data/tests/publishing_direct.rs`, `backend/crates/data/tests/publishing_jobs.rs`

**Why:** Several tests run two calls with `tokio::join!` over separately acquired connections, or signal readiness before the contended statement. That does not force the calls to overlap, so a version of the code with no lock can still pass.

- [ ] **Step 1: Move** `wait_for_blocked_statement` from `publishing_review.rs` to `tests/common/mod.rs`, and add a `within(fut)` helper there that wraps `tokio::time::timeout(Duration::from_secs(5), fut)` and panics with "blocked: the lock under test is not skipped" on timeout.
- [ ] **Step 2: Rewrite** each test below as the table says.

| Test | File | Pattern | Mutation that must make it fail |
| --- | --- | --- | --- |
| `claim_once` | `publishing_deliveries.rs` | A claims in an open transaction; B claims in a spawned task; `wait_for_blocked_statement`; A commits; B returns `None` | remove `d.state = 'pending'` from `DeliveryRepo::claim`: after the recheck both claims win. (Removing the `dispatch_gen` condition changes nothing; both callers carry generation 0.) |
| `overdue_pending_redispatched_once` | `publishing_sweep.rs` | two transactions in one task: A sweeps and stays open, B sweeps under `within` and gets nothing | drop `SKIP LOCKED`: B blocks on A's row and the timeout fails it. It never double-dispatches (on the recheck `updated_at <= cutoff` is false), so blocking is the only thing to observe |
| `two_workers_claim_disjoint_batches` | `publishing_direct.rs` | two transactions in one task, `cap = None` (with a cap, the quota row serializes claims of one owner): A claims and stays open, B claims under `within`, row sets disjoint | drop `SKIP LOCKED` from the row sub-select: B blocks. Change the delivery `FOR SHARE` to `FOR UPDATE`: B blocks |
| `edit_racing_job_creation` | `publishing_jobs.rs` | today the oneshot fires before `PostRepo::lock`, so tx1 can commit before tx2 asks for the lock. Replace the oneshot with `wait_for_blocked_statement` before `tx1.commit()`, and wrap it in `within` | remove `FOR UPDATE` from `PostRepo::lock`: the editor never blocks, the wait times out |

  `two_sweeps_never_list_one_orphan_twice` (Task 2) already follows this pattern. `two_ticks_advance_once` and the `JoinSet` quota tests need no change: the first already forces its interleaving, and the others prove a single atomic statement.
- [ ] **Step 3: Mutation check.** For each row of the table, apply the mutation, confirm the test fails (not hangs), and restore the code. Record the four results in the commit message.
- [ ] **Step 4: Run** `cargo test -p postit-data` on a quiet machine. Expected: PASS.
- [ ] **Step 5: Commit** `test(data): force the interleavings the publishing concurrency tests claim`.

---

### Task 6: Gates and handoff

- [ ] **Step 1: Run**, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo sqlx prepare --check --workspace`, `cargo xtask openapi --check`. Expected: all pass. (`postit-data`'s `tests/jobs.rs` has been seen failing with `PoolTimedOut` when the whole workspace runs at once and passing alone; if it is the only failure, rerun it alone and say so.)
- [ ] **Step 2:** Update the job map in `publishing/mod.rs`. `direct_send_batch` calls `claim_batch`, `renew_batch`, `record`, `wait_for_cap` (when a claim returns `CapReached`), and `finish_if_done`, and stops on `BatchClaim::Stale`. `delivery_due_sweep` passes `sweep_grace` to `orphaned_direct`.
- [ ] **Step 3: Commit** `docs(data): update the publishing job map for the follow-up`.

---

## Notes for D1

- A batch job carries the delivery and its dispatch generation and builds the `Fence` from them. On `CapReached` it computes `resume_at` from the cap period, calls `wait_for_cap`, and enqueues the next batch at `resume_at`.
- A batch job enqueues its successor in the same transaction as its last `record`, so the gap with no live lease stays well inside `sweep_grace`. The sweep still lists a delivery whose gap runs past the grace; a duplicate batch job is harmless (claims are disjoint), and the stamp limits it to one per grace.
- `publishing.processing_timeout` exempts direct deliveries while rows are queued (plan 03, "Processing"); a delivery waiting for its cap has queued rows.
- The lease passed to `claim_batch` and `renew_batch` must exceed the time one batch takes at the transport's rate, or the worker renews between sends.

## Out of scope

- Lock-order changes to the post edit path versus the recurrence tick. Both lock a post and a recurrence in opposite orders; that belongs with the recurring-posts UI work (D5), where the edit path is written.
- Anything in D1 itself (the worker honoring `cancel_requested_at`, enqueue ordering).
- `cancel_for_account` cancels only `pending` and `parked` deliveries. Whether disconnecting an email account stops a running campaign, or lets its batches fail on the missing credentials, is decided with account disconnection in D1.
