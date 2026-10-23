# B2b Follow-up: Direct-Send Hardening and Concurrency Proofs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the review findings of `docs/superpowers/plans/2026-10-19-b2b-publishing-concurrency.md` that were not fixed with that plan, so D1 builds on a direct-send path that cannot be misused or spin, and the concurrency tests actually force the interleavings they claim to cover.

**Architecture:** Everything stays in `postit-data::publishing` (`direct`, `sweep`) and its tests. No new module, no migration file (the single-migration policy still holds; if a task needs a schema change it edits `0001_init.sql`).

**Tech Stack:** Rust 2024, sqlx 0.9, PostgreSQL 18, `tokio::sync::Barrier`.

**Spec:** `docs/superpowers/specs/2026-10-19-b2-domain-schema-design.md` (sections "Publish jobs and deliveries" through "Quotas" and "Sending an email campaign"), arguing from `!ref/plans/03. publishing roadmap.md` phase B2 and D1. This plan runs after B2b and before D1.

## Where these items come from

The B2b whole-branch review found four Important issues and a list of minors. The Important ones and the cheap minors were fixed in B2b (commits `d696c03` and the commit after it). What remains is below. Each item needs a decision or a larger change than a review fix should carry.

## Global Constraints

- Every Global Constraint of the B2b plan applies: lints, single migration, compile-checked queries with `cargo sqlx prepare --workspace`, `#[sqlx::test]`, quality gates after every task, commit format, American English.
- Concurrency tests never sleep to order events. They use `tokio::sync::Barrier` or `oneshot`, with the contended statements running inside transactions that stay open until both sides have started.
- A signature change in `DirectRepo` or `Sweep` updates every caller in this repository in the same commit.

## Review Focus

1. **A campaign that hits the daily cap.** It must stop cleanly and resume the next period. Nothing may re-enqueue a no-op batch on every sweep while it waits.
2. **Two workers on one direct delivery after a lease expiry.** A worker that lost its batch must not be able to record results onto rows another worker now holds.
3. **A crashed batch.** The units it reserved stay counted, and a reclaimed batch reserves again. This is the intended, safe direction; it must be documented and pinned by a test, not left implicit.
4. **The concurrency tests named in Task 4.** Each must fail if the code under test loses its lock or fence, which means the test has to hold the contended rows open while the second caller runs.

---

### Task 1: Batch numbers allocated by the data layer

**Files:**
- Modify: `server/crates/data/src/publishing/direct.rs`
- Test: `server/crates/data/tests/publishing_direct.rs`

**Why:** `DirectRepo::record` is conditioned on the batch number the row was leased to, but the caller chooses that number. If D1 reuses a number across workers (a per-delivery counter that resets after a restart, say), a stale worker's `record` lands on rows another worker now holds.

**Interfaces:**
- Changes: `DirectRepo::claim_batch(conn, delivery, size, now, lease, cap) -> Result<BatchClaim, DataError>` drops the `batch: i32` argument. `BatchClaim::Rows` becomes `Rows { batch: i32, rows: Vec<DirectSendRow> }`.
- The batch number is `COALESCE(MAX(batch), 0) + 1` over the delivery's rows, computed after the claim takes `SELECT … FROM deliveries WHERE id = $1 FOR UPDATE`. That lock also orders concurrent claims of one delivery; it takes the delivery row before any job row, as `apply` does.

- [ ] **Step 1: Write failing tests** (change the existing calls to the new signature first; they then fail to compile, which is the red state)

```rust
#[sqlx::test] async fn batch_numbers_are_unique_per_claim() // two claims, including after a lease expiry, never share a number
#[sqlx::test] async fn a_worker_that_lost_its_batch_cannot_record() // claim (batch 1), expire the lease, second claim takes the rows (batch 2), record with batch 1 returns false and changes nothing
#[sqlx::test] async fn concurrent_claims_get_different_numbers() // two transactions, Barrier before the claim, numbers differ, row sets disjoint
```

- [ ] **Step 2: Run** `cargo test -p postit-data --test publishing_direct`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, then `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `fix(data): allocate direct-send batch numbers in the data layer`.

---

### Task 2: A cap-blocked campaign is parked, not re-listed

**Files:**
- Modify: `server/crates/data/src/publishing/sweep.rs`, `server/crates/data/src/publishing/direct.rs`, and `server/crates/core/src/delivery.rs` only if the transition below is missing
- Test: `server/crates/data/tests/publishing_sweep.rs`, `server/crates/data/tests/publishing_direct.rs`

**Why:** A `processing` direct delivery that stopped at `CapReached` has queued rows and no live lease, so `Sweep::orphaned_direct` lists it on every run until the cap resets, and D1 would enqueue a no-op batch each time. The same query also lists a delivery that was materialized a moment ago and has no lease yet.

**Interfaces:**
- Changes: `Sweep::orphaned_direct(conn, now, grace, limit)` gains `grace: Duration` and skips deliveries whose `updated_at` is newer than `now - grace`.
- Adds: `DirectRepo::park_for_cap(conn, Fence, resume_at: DateTime<Utc>, now) -> Result<Applied, DataError>`. It applies `Park(QuotaExceeded)` from `processing` with `next_attempt_at = resume_at`, so `Sweep::unpark` resumes it once the next period starts. Check `postit_core::transition` first: if `(Processing, Park(QuotaExceeded))` is not allowed, add it in core with its table test, as its own commit, before this task.

- [ ] **Step 1: Write failing tests**

```rust
#[sqlx::test] async fn orphaned_direct_skips_a_delivery_touched_inside_the_grace() // materialized just now, no lease -> not listed; same delivery aged past grace -> listed
#[sqlx::test] async fn cap_reached_campaign_parks_and_resumes_next_period() // CapReached, park_for_cap, not listed by orphaned_direct, unpark after resume_at returns it to pending with queued rows intact
#[sqlx::test] async fn park_for_cap_is_fenced() // stale dispatch_gen -> Applied::Stale
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, then `cargo sqlx prepare --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `fix(data): park cap-blocked direct deliveries instead of re-listing them`.

---

### Task 3: Pin and document the crashed-batch cap behavior

**Files:**
- Modify: `server/crates/data/src/publishing/direct.rs` (module and `claim_batch` rustdoc)
- Test: `server/crates/data/tests/publishing_direct.rs`

**Why:** A batch whose worker died after reserving daily-cap units keeps those units, and reclaiming its rows reserves again. This is deliberate (some of that mail may have been sent but not recorded), but it is only a ledger line today.

- [ ] **Step 1: Write the test**

```rust
#[sqlx::test] async fn a_reclaimed_batch_counts_against_the_cap_again() // cap 10, claim 5 (used 5), lease expires, claim again -> used 10, not 5; the doc comment names this
```

- [ ] **Step 2: Run** it. Expected: PASS already, since it pins existing behavior. If it fails, the behavior is not what the B2b ledger says; stop and report.
- [ ] **Step 3:** Add the rustdoc: what is over-counted, why that direction is safe, and that a user may see their cap used up after a crash. Add one sentence to `docs/` where the email daily cap is described, if such a page exists.
- [ ] **Step 4: Commit** `docs(data): document how a crashed direct-send batch counts against the cap`.

---

### Task 4: Concurrency tests that force the interleaving

**Files:**
- Modify: `server/crates/data/tests/publishing_deliveries.rs`, `server/crates/data/tests/publishing_sweep.rs`, `server/crates/data/tests/publishing_direct.rs`, `server/crates/data/tests/publishing_review.rs`

**Why:** Several tests run two calls with `tokio::join!` over separately acquired connections. That does not force the calls to overlap, so a version of the code with no lock can still pass.

- [ ] **Step 1: Identify** the tests: the two-claims-one-winner test in `publishing_deliveries.rs`, `overdue_pending_redispatched_once` in `publishing_sweep.rs`, and `two_workers_claim_disjoint_batches` in `publishing_direct.rs`.
- [ ] **Step 2: Rewrite each** so both sides run in their own spawned task and transaction, wait on a `Barrier` after beginning the transaction, and commit only after both have finished the statement under test. The overdue-sweep and batch-claim tests use `FOR UPDATE SKIP LOCKED`, so they also assert that the second caller returned while the first still held its rows (`Barrier` between the two statements).
- [ ] **Step 3: Mutation check.** For each rewritten test, temporarily remove the lock or fence in the code under test (drop `FOR UPDATE SKIP LOCKED`, drop the `dispatch_gen` condition) and confirm the test now fails. Restore the code. Record the three results in the commit message.
- [ ] **Step 4: Run** `cargo test -p postit-data` on a quiet machine. Expected: PASS.
- [ ] **Step 5: Commit** `test(data): force the interleavings the publishing concurrency tests claim`.

---

### Task 5: Gates and handoff

- [ ] **Step 1: Run**, from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo sqlx prepare --check --workspace`, `cargo xtask openapi --check`. Expected: all pass. (`postit-data`'s `tests/jobs.rs` has been seen failing with `PoolTimedOut` when the whole workspace runs at once and passing alone; if it is the only failure, rerun it alone and say so.)
- [ ] **Step 2:** Update the module rustdoc of `publishing/mod.rs` if `claim_batch`, `orphaned_direct`, or `park_for_cap` changed what a D1 job calls. D1 now also calls `DirectRepo::park_for_cap` when a claim returns `CapReached`.
- [ ] **Step 3: Commit** `docs(data): update the publishing job map for the follow-up`.

---

## Out of scope

- Lock-order changes to the post edit path versus the recurrence tick. Both lock a post and a recurrence in opposite orders; that belongs with the recurring-posts UI work (D5), where the edit path is written.
- Anything in D1 itself (the worker honoring `cancel_requested_at`, enqueue ordering).
