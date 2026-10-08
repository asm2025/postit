# P8: Admin job console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An admin-only, Hangfire-style job console: a postit-owned job ledger, `JobConsole` in `postit-jobs`, eight `/api/v1/admin/jobs*` endpoints, and a Jobs section in the web app.

**Architecture:** Two new tables (`jobs`, `job_attempts`) in `postit-data` record every job and every try. `JobQueue` writes the ledger row in the caller's transaction at enqueue time; `dispatch` is the only writer of attempt transitions. `JobConsole` reads and acts through repositories only, never touching `apalis.*`. The API wraps it behind `RequireAdmin`; the web app polls it.

**Tech Stack:** Rust 2024 (axum, utoipa, sqlx compile-time queries, emixdb pagination), apalis (unchanged, execution only), React 19 + TanStack Query + shadcn + `recharts`, pnpm.

**Spec:** `docs/superpowers/specs/2026-10-08-p8-job-console-design.md` (read it first; plan 02 `!ref/plans/02. foundation.md` phase P8 governs where the spec is silent).

## Global Constraints

- Quality gates must pass after every Rust task: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (run from `server/`).
- Web gates after every web task (PowerShell, from `web/`): `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test`. **pnpm only, never npm; the Git Bash pnpm shim is broken, use PowerShell.**
- Single-migration policy: all schema lives in one migration file under `server/crates/data/migrations/`; never add a second numbered file.
- Regenerate the sqlx cache after any `query!` change: from `server/`, `cargo sqlx prepare --workspace` (CI checks `cargo sqlx prepare --check --workspace`).
- No apalis type may appear in another crate's public API. The console never reads or writes `apalis.*`.
- Job payloads hold IDs only. The console never returns a payload. Error messages are stored and returned only for job types that opt in (`show_error_message`).
- Every console action writes its audit event (`job_retried`, `job_cancelled`, `job_deleted`, `job_triggered`) in the same transaction as the action.
- `/api/v1` changes are additive only. `X-Postit-Act-As` is rejected on admin routes (the existing extractors already do this).
- Use maintained libraries over hand-rolled code (charts: `recharts`).
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Code style: match surrounding code (doc comments on public items explaining behavior, `# Errors` sections, `unwrap_or_else(|e| unreachable!(...))` in tests, no `unwrap()`/`expect()` outside tests).

## Spec amendments decided while planning

1. **`error_code`** is populated from the failure kind where the kind implies one (`retries_exhausted` → `max_attempts`, `unknown_type` → `not_registered`, `panic` → `handler_panic`, `interrupted` → `worker_lost`); it is null for plain `retry`/`fatal` errors, because `JobError` carries only a message. The column stays so handlers can supply codes later.
2. **Retry of a `failed` job** (a try failed and a retry is waiting): the console cancels the pending retry and creates the new job in one transaction, so the work cannot run twice. `cancel` is therefore allowed from `queued`, `scheduled` and `failed`.
3. **Registration API is unchanged.** Console metadata lives in a separate `ConsoleCatalog` attached to the registry (`JobRegistry::with_console`), so none of the ~20 existing `register(...)` call sites change, and the `api` role (which builds no handlers) builds the catalog on its own.
4. **`max_attempts`** on `jobs` is filled on the first try (the enqueue path does not know the retry policy), so it is null for jobs that have not started.

## Review Focus

- A cancelled job whose apalis row is already relayed: the handler must never run (Task 6).
- A worker that dies mid-try: the open attempt becomes `interrupted` on the next try, never stays "running" forever (Task 6).
- Two admins acting on one job at once (retry vs cancel, double retry): exactly one wins, the other gets 409 (Tasks 3, 8, 11).
- A job type that did not opt in: an error message containing an address or SMTP detail is neither stored nor returned (Tasks 6, 7, 11).
- A job enqueued inside a rolled-back transaction leaves no ledger row (Task 5).
- Ledger write failure must not change whether a job runs (Task 6).
- `from > to`, unknown `state`, page 0 / page_size 101 on the list endpoint: 422, not 500 (Task 10).
- An unknown job ID or recurring name: 404, not 500 (Tasks 10, 11).
- Members and pending users: 403 on every route (Task 10).

---

## File Structure

**Create**
- `server/crates/data/src/jobs.rs` — ledger types and `JobsRepo` (writes, guarded transitions, reads, stats, purge).
- `server/crates/data/tests/jobs.rs` — repository tests.
- `server/crates/jobs/src/console_spec.rs` — `ConsoleSpec`, `ConsoleCatalog`, `JobSummary`.
- `server/crates/jobs/src/ledger.rs` — dispatch-side ledger recording (`run_recorded`).
- `server/crates/jobs/src/console.rs` — `JobConsole`, `ConsoleError`, read models.
- `server/crates/jobs/tests/ledger.rs`, `server/crates/jobs/tests/console.rs` — integration tests.
- `server/crates/api/src/routes/jobs.rs` — the eight handlers.
- `server/crates/api/src/dto_jobs.rs` — job DTOs.
- `server/crates/api/tests/jobs.rs` — API tests.
- `web/src/features/jobs/` — `queries.ts`, `JobsDashboard.tsx`, `JobsList.tsx`, `JobDetailPage.tsx`, `RecurringPage.tsx`, `JobState.tsx`, `labels.ts`, and `*.test.tsx`.

**Modify**
- `server/crates/data/migrations/` — consolidate `0001`–`0007` into `0001_init.sql`, add ledger tables.
- `server/crates/data/src/lib.rs`, `audit.rs`, `recurring_runs.rs`.
- `server/crates/jobs/src/{lib,registry,queue,recurring,dispatch,backend,maintenance,relay}.rs`.
- `server/crates/identity/src/jobs.rs`, `server/crates/mail/src/{lib,send}.rs` — `console` functions.
- `server/crates/api/src/{lib,state,error,openapi,testkit,dto}.rs`, `routes/mod.rs`.
- `server/crates/server/src/{compose,lib}.rs`.
- `api/openapi.json`, `web/src/api/schema.d.ts` (generated), `web/src/{routes.tsx,components/Shell.tsx,api/errors.ts}`, `web/package.json`.
- `CLAUDE.md` (project state line), `!ref/plans/02. foundation.md` (record amendments).

---

### Task 1: Consolidate migrations into one file

**Files:**
- Create: `server/crates/data/migrations/0001_init.sql`
- Delete: `server/crates/data/migrations/0001_citext.sql` … `0007_job_recurring_runs.sql`

**Interfaces:**
- Produces: a single migration whose resulting schema is identical to the seven files combined. Tasks 2+ append ledger tables to this file.

- [ ] **Step 1: Capture the reference schema from the seven files**

Run from `D:\Work\rust\postit` (Git Bash). Needs the dev Postgres from `./stack.sh up development` (container `postit-postgres`); the commands use throwaway databases and drop them.

```bash
cd /d/Work/rust/postit
docker exec postit-postgres psql -U postgres -c "DROP DATABASE IF EXISTS mig_old" -c "CREATE DATABASE mig_old"
for f in server/crates/data/migrations/000*.sql; do
  docker exec -i postit-postgres psql -U postgres -d mig_old -v ON_ERROR_STOP=1 < "$f" || break
done
docker exec postit-postgres pg_dump -U postgres -s --no-owner mig_old > "$CLAUDE_JOB_DIR/tmp/old.sql"
```

Expected: no psql errors; `old.sql` is non-empty.

- [ ] **Step 2: Build the consolidated file**

Concatenate in numeric order, keeping each file's content verbatim and separating with a comment line, then fold any `ALTER TABLE` on a table created earlier into its `CREATE TABLE`:

```bash
out=server/crates/data/migrations/0001_init.sql.new
: > "$out"
for f in server/crates/data/migrations/000*.sql; do
  printf '%s\n' "-- from $(basename "$f")" >> "$out"
  cat "$f" >> "$out"
  printf '\n' >> "$out"
done
grep -n "ALTER TABLE" "$out" || echo "no ALTER statements"
```

Expected: `no ALTER statements` (from the files read so far, every table is created in one `CREATE TABLE`). If `ALTER` lines appear, edit the `.new` file to fold them into the `CREATE TABLE` they modify, with no `DROP` or re-`CREATE` of columns or indexes.

- [ ] **Step 3: Swap the files and verify identical schema**

```bash
cd server/crates/data/migrations
git rm -q 0001_citext.sql 0002_users.sql 0003_audit_events.sql 0004_user_preferences.sql 0005_idempotency_keys.sql 0006_job_outbox.sql 0007_job_recurring_runs.sql
mv 0001_init.sql.new 0001_init.sql
cd /d/Work/rust/postit
docker exec postit-postgres psql -U postgres -c "DROP DATABASE IF EXISTS mig_new" -c "CREATE DATABASE mig_new"
docker exec -i postit-postgres psql -U postgres -d mig_new -v ON_ERROR_STOP=1 < server/crates/data/migrations/0001_init.sql
docker exec postit-postgres pg_dump -U postgres -s --no-owner mig_new > "$CLAUDE_JOB_DIR/tmp/new.sql"
diff "$CLAUDE_JOB_DIR/tmp/old.sql" "$CLAUDE_JOB_DIR/tmp/new.sql" && echo SCHEMA-IDENTICAL
docker exec postit-postgres psql -U postgres -c "DROP DATABASE mig_old" -c "DROP DATABASE mig_new"
```

Expected: `SCHEMA-IDENTICAL` (a diff only in the `-- Dumped` header lines is fine; anything else must be fixed).

- [ ] **Step 4: Reset the dev database and rebuild the sqlx cache**

The migration history table now has a different checksum, so the dev database must be reset (`./stack.sh down -v` wipes the DB and the cached Zitadel key; the alternative is to drop only the `postit` database). Then, from `server/`:

```bash
cargo sqlx prepare --workspace
cargo test --workspace
```

Expected: tests pass (`#[sqlx::test(migrations = "../data/migrations")]` applies the single file); `git status` shows no `.sqlx` changes (no query text changed).

- [ ] **Step 5: Commit**

```bash
git add -A server/crates/data/migrations
git commit -m "refactor(data): consolidate migrations into one file

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Ledger tables and write-side repository

**Files:**
- Modify: `server/crates/data/migrations/0001_init.sql` (append)
- Create: `server/crates/data/src/jobs.rs`
- Modify: `server/crates/data/src/lib.rs` (add `pub mod jobs;`)
- Test: `server/crates/data/tests/jobs.rs`

**Interfaces:**
- Produces (all in `postit_data::jobs`):

```rust
pub enum JobState { Queued, Running, Succeeded, Failed, Dead, Killed, Cancelled }
pub enum AttemptOutcome { Succeeded, Retrying, Failed, Killed, Interrupted, Cancelled }
pub enum ErrorKind { Retry, RetriesExhausted, Fatal, UnknownType, Panic, Interrupted }
// each: fn as_str(self) -> &'static str, impl FromStr<Err = ParseEnumError>

pub struct NewJob<'a> {
    pub id: Uuid, pub job_type: &'a str, pub queue: &'a str, pub payload: &'a Value,
    pub run_at: DateTime<Utc>, pub recurring_name: Option<&'a str>, pub retried_from: Option<Uuid>,
}
pub enum Begin { Run, Cancelled, Untracked }
pub struct AttemptError<'a> { pub kind: ErrorKind, pub code: Option<&'a str>, pub message: Option<&'a str> }
pub struct Finish<'a> {
    pub outcome: AttemptOutcome, pub state: JobState,
    pub next_run_at: Option<DateTime<Utc>>, pub error: Option<AttemptError<'a>>,
}
impl JobsRepo {
    pub async fn insert(conn: &mut PgConnection, job: &NewJob<'_>) -> Result<(), DataError>;
    pub async fn begin_attempt(conn: &mut PgConnection, id: Uuid, attempt: i32, worker: &str, max_attempts: Option<i32>) -> Result<Begin, DataError>;
    pub async fn finish_attempt(conn: &mut PgConnection, id: Uuid, attempt: i32, finish: &Finish<'_>) -> Result<(), DataError>;
    pub async fn cancel(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError>;       // queued|failed -> cancelled
    pub async fn delete_finished(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError>; // succeeded|dead|killed|cancelled
}
```

- [ ] **Step 1: Append the tables to the migration**

Append to `server/crates/data/migrations/0001_init.sql`:

```sql
-- from the P8 job ledger: one row per job, one row per try. `jobs.id` equals the
-- `job_outbox` id and the job storage's task id.
CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    job_type TEXT NOT NULL,
    queue TEXT NOT NULL,
    payload JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'dead', 'killed', 'cancelled')),
    run_at TIMESTAMPTZ NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER,
    recurring_name TEXT,
    retried_from UUID REFERENCES jobs (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
CREATE INDEX jobs_state_run_at_idx ON jobs (state, run_at);
CREATE INDEX jobs_type_created_at_idx ON jobs (job_type, created_at);
CREATE INDEX jobs_finished_at_idx ON jobs (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX jobs_recurring_idx ON jobs (recurring_name, created_at) WHERE recurring_name IS NOT NULL;
CREATE INDEX jobs_retried_from_idx ON jobs (retried_from) WHERE retried_from IS NOT NULL;

CREATE TABLE job_attempts (
    job_id UUID NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    worker TEXT NOT NULL,
    outcome TEXT
        CHECK (outcome IN ('succeeded', 'retrying', 'failed', 'killed', 'interrupted', 'cancelled')),
    error_kind TEXT
        CHECK (error_kind IN ('retry', 'retries_exhausted', 'fatal', 'unknown_type', 'panic', 'interrupted')),
    error_code TEXT,
    -- Stored only for job types that opted in to messages; null otherwise.
    error_message TEXT,
    PRIMARY KEY (job_id, attempt)
);
CREATE INDEX job_attempts_finished_at_idx ON job_attempts (finished_at) WHERE finished_at IS NOT NULL;
```

- [ ] **Step 2: Write the failing tests**

Create `server/crates/data/tests/jobs.rs` (look at `server/crates/data/tests/recurring_runs.rs` for the file's `conn` helper pattern and copy it):

```rust
use chrono::{Duration, Utc};
use postit_data::jobs::{
    AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobState, JobsRepo, NewJob,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn new_job<'a>(id: Uuid, payload: &'a serde_json::Value) -> NewJob<'a> {
    NewJob {
        id,
        job_type: "send_email",
        queue: "mail",
        payload,
        run_at: Utc::now(),
        recurring_name: None,
        retried_from: None,
    }
}

async fn state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("state: {e}"))
}

#[sqlx::test(migrations = "migrations")]
async fn a_new_job_is_queued_with_no_attempts(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    JobsRepo::insert(&mut conn, &new_job(id, &json!({"recipient": "x"})))
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    assert_eq!(state(&pool, id).await, "queued");
}

#[sqlx::test(migrations = "migrations")]
async fn begin_then_finish_records_one_attempt(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w1", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    assert!(matches!(begin, Begin::Run));
    assert_eq!(state(&pool, id).await, "running");
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish { outcome: AttemptOutcome::Succeeded, state: JobState::Succeeded, next_run_at: None, error: None },
    )
    .await
    .unwrap_or_else(|e| unreachable!("finish: {e}"));
    assert_eq!(state(&pool, id).await, "succeeded");
    let finished: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = $1 AND finished_at IS NOT NULL")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(finished, 1);
}

#[sqlx::test(migrations = "migrations")]
async fn a_cancelled_job_is_not_run(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(JobsRepo::cancel(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}")));
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(begin, Begin::Cancelled));
}

#[sqlx::test(migrations = "migrations")]
async fn cancel_is_refused_while_running(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!JobsRepo::cancel(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}")));
}

#[sqlx::test(migrations = "migrations")]
async fn a_new_try_closes_an_open_attempt_as_interrupted(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    // The worker dies; the storage re-fetches the task as attempt 2.
    JobsRepo::begin_attempt(&mut conn, id, 2, "w2", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM job_attempts WHERE job_id = $1 AND attempt = 1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outcome, "interrupted");
}

#[sqlx::test(migrations = "migrations")]
async fn a_missing_ledger_row_is_untracked_not_an_error(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let begin = JobsRepo::begin_attempt(&mut conn, Uuid::now_v7(), 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(begin, Begin::Untracked));
}

#[sqlx::test(migrations = "migrations")]
async fn a_retry_waiting_job_is_failed_not_finished_and_stores_no_message_by_default(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", Some(3)).await.unwrap_or_else(|e| unreachable!("{e}"));
    let next = Utc::now() + Duration::seconds(60);
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish {
            outcome: AttemptOutcome::Retrying,
            state: JobState::Failed,
            next_run_at: Some(next),
            error: Some(AttemptError { kind: ErrorKind::Retry, code: None, message: None }),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let message: Option<String> = sqlx::query_scalar("SELECT error_message FROM job_attempts WHERE job_id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(message, None);
    assert_eq!(state(&pool, id).await, "failed");
    let finished: Option<chrono::DateTime<Utc>> = sqlx::query_scalar("SELECT finished_at FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(finished, None);
}

#[sqlx::test(migrations = "migrations")]
async fn delete_finished_refuses_a_live_job(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload)).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!JobsRepo::delete_finished(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}")));
    JobsRepo::cancel(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(JobsRepo::delete_finished(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}")));
}
```

Check how the other tests in `server/crates/data/tests/` reference migrations (`migrations = "migrations"` vs a relative path) and use the same attribute.

- [ ] **Step 3: Run the tests to verify they fail**

Run (from `server/`): `cargo test -p postit-data --test jobs`
Expected: FAIL to compile — `postit_data::jobs` does not exist.

- [ ] **Step 4: Implement `jobs.rs` (types and writes)**

Create `server/crates/data/src/jobs.rs`. Follow `recurring_runs.rs` for style (`sqlx::query!`, `# Errors` docs). Enum boilerplate (shown once; apply the same shape to `AttemptOutcome` and `ErrorKind` with their strings from the migration `CHECK`s):

```rust
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::users::ParseEnumError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState { Queued, Running, Succeeded, Failed, Dead, Killed, Cancelled }

impl JobState {
    pub const ALL: [Self; 7] = [Self::Queued, Self::Running, Self::Succeeded, Self::Failed, Self::Dead, Self::Killed, Self::Cancelled];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued", Self::Running => "running", Self::Succeeded => "succeeded",
            Self::Failed => "failed", Self::Dead => "dead", Self::Killed => "killed", Self::Cancelled => "cancelled",
        }
    }

    /// A state a job never leaves on its own.
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Dead | Self::Killed | Self::Cancelled)
    }
}

impl std::str::FromStr for JobState {
    type Err = ParseEnumError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|s| s.as_str() == value).ok_or_else(|| ParseEnumError(value.to_string()))
    }
}
```

`AttemptOutcome`: `Succeeded "succeeded"`, `Retrying "retrying"`, `Failed "failed"`, `Killed "killed"`, `Interrupted "interrupted"`, `Cancelled "cancelled"`. `ErrorKind`: `Retry "retry"`, `RetriesExhausted "retries_exhausted"`, `Fatal "fatal"`, `UnknownType "unknown_type"`, `Panic "panic"`, `Interrupted "interrupted"`; add `ErrorKind::default_code(self) -> Option<&'static str>` returning `RetriesExhausted → Some("max_attempts")`, `UnknownType → Some("not_registered")`, `Panic → Some("handler_panic")`, `Interrupted → Some("worker_lost")`, others `None`.

Structs and writes:

```rust
pub struct NewJob<'a> {
    pub id: Uuid,
    pub job_type: &'a str,
    pub queue: &'a str,
    pub payload: &'a Value,
    pub run_at: DateTime<Utc>,
    pub recurring_name: Option<&'a str>,
    pub retried_from: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// Run the handler.
    Run,
    /// An admin cancelled the job; finish it without running the handler.
    Cancelled,
    /// No ledger row (the job predates the ledger or was pushed to the storage directly):
    /// run the handler without recording.
    Untracked,
}

pub struct AttemptError<'a> {
    pub kind: ErrorKind,
    pub code: Option<&'a str>,
    /// Pass `Some` only for job types that opted in to error messages.
    pub message: Option<&'a str>,
}

pub struct Finish<'a> {
    pub outcome: AttemptOutcome,
    pub state: JobState,
    /// Set for `Failed` (a retry is waiting): when it becomes due.
    pub next_run_at: Option<DateTime<Utc>>,
    pub error: Option<AttemptError<'a>>,
}

pub struct JobsRepo;

impl JobsRepo {
    pub async fn insert(conn: &mut PgConnection, job: &NewJob<'_>) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO jobs (id, job_type, queue, payload, run_at, recurring_name, retried_from)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            job.id, job.job_type, job.queue, job.payload, job.run_at, job.recurring_name, job.retried_from,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    pub async fn begin_attempt(
        conn: &mut PgConnection,
        id: Uuid,
        attempt: i32,
        worker: &str,
        max_attempts: Option<i32>,
    ) -> Result<Begin, DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        let state: Option<String> =
            sqlx::query_scalar!("SELECT state FROM jobs WHERE id = $1 FOR UPDATE", id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(state) = state else {
            tx.rollback().await?;
            return Ok(Begin::Untracked);
        };
        if state == "cancelled" {
            tx.commit().await?;
            return Ok(Begin::Cancelled);
        }
        sqlx::query!(
            "UPDATE job_attempts SET finished_at = now(), outcome = 'interrupted',
                    error_kind = 'interrupted', error_code = 'worker_lost'
             WHERE job_id = $1 AND finished_at IS NULL AND attempt <> $2",
            id, attempt,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO job_attempts (job_id, attempt, worker) VALUES ($1, $2, $3)
             ON CONFLICT (job_id, attempt)
             DO UPDATE SET started_at = now(), finished_at = NULL, outcome = NULL,
                           error_kind = NULL, error_code = NULL, error_message = NULL, worker = $3",
            id, attempt, worker,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE jobs SET state = 'running', attempts = GREATEST(attempts, $2),
                    max_attempts = COALESCE($3, max_attempts), started_at = now()
             WHERE id = $1",
            id, attempt, max_attempts,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Begin::Run)
    }

    pub async fn finish_attempt(
        conn: &mut PgConnection,
        id: Uuid,
        attempt: i32,
        finish: &Finish<'_>,
    ) -> Result<(), DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        let (kind, code, message) = match &finish.error {
            Some(e) => (Some(e.kind.as_str()), e.code, e.message),
            None => (None, None, None),
        };
        sqlx::query!(
            "UPDATE job_attempts SET finished_at = now(), outcome = $3,
                    error_kind = $4, error_code = $5, error_message = $6
             WHERE job_id = $1 AND attempt = $2",
            id, attempt, finish.outcome.as_str(), kind, code, message,
        )
        .execute(&mut *tx)
        .await?;
        // `finished_at` only for a state the job never leaves: a `failed` job is waiting
        // for its retry, so it keeps `finished_at` null (stats, purge and the list's time
        // filter all treat `finished_at` as "terminal since").
        sqlx::query!(
            "UPDATE jobs SET state = $2,
                    finished_at = CASE WHEN $4 THEN now() END,
                    run_at = COALESCE($3, run_at)
             WHERE id = $1",
            id, finish.state.as_str(), finish.next_run_at, finish.state.is_finished(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `queued` or `failed` (a retry is waiting) -> `cancelled`. Returns whether this call
    /// changed the row; `false` means the job is running, finished, or gone.
    pub async fn cancel(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError> {
        let n = sqlx::query!(
            "UPDATE jobs SET state = 'cancelled', finished_at = now()
             WHERE id = $1 AND state IN ('queued', 'failed')",
            id,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    pub async fn delete_finished(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError> {
        let n = sqlx::query!(
            "DELETE FROM jobs WHERE id = $1 AND state IN ('succeeded', 'dead', 'killed', 'cancelled')",
            id,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(n == 1)
    }
}
```

Add `pub mod jobs;` to `data/src/lib.rs`. Note: `begin_attempt` and `finish_attempt` start their own transaction on the passed connection (sqlx nests as a savepoint if `conn` is already inside one).

- [ ] **Step 5: Run the tests and regenerate the cache**

Run (from `server/`): `cargo sqlx prepare --workspace` then `cargo test -p postit-data --test jobs`
Expected: PASS (8 tests).

- [ ] **Step 6: Gates and commit**

Run the Global gates. Then:

```bash
git add server/crates/data .sqlx server/.sqlx 2>/dev/null; git add -A server
git commit -m "feat(data): job ledger tables and write-side repository

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Ledger read-side repository (list, detail, stats, purge)

**Files:**
- Modify: `server/crates/data/src/jobs.rs`, `server/crates/data/src/recurring_runs.rs`
- Test: `server/crates/data/tests/jobs.rs`

**Interfaces:**
- Consumes: Task 2 types.
- Produces:

```rust
pub struct JobRow {
    pub id: Uuid, pub job_type: String, pub queue: String, pub payload: Value, pub state: JobState,
    pub run_at: DateTime<Utc>, pub attempts: i32, pub max_attempts: Option<i32>,
    pub recurring_name: Option<String>, pub retried_from: Option<Uuid>,
    pub created_at: DateTime<Utc>, pub started_at: Option<DateTime<Utc>>, pub finished_at: Option<DateTime<Utc>>,
    pub last_error_kind: Option<String>, pub last_error_code: Option<String>,
}
pub struct AttemptRow {
    pub attempt: i32, pub started_at: DateTime<Utc>, pub finished_at: Option<DateTime<Utc>>, pub worker: String,
    pub outcome: Option<String>, pub error_kind: Option<String>, pub error_code: Option<String>, pub error_message: Option<String>,
}
#[derive(Default)]
pub struct JobFilter { pub state: Option<String>, pub job_type: Option<String>, pub from: Option<DateTime<Utc>>, pub to: Option<DateTime<Utc>> }
pub struct StateCount { pub state: String, pub count: i64 }
pub struct TypeCount { pub job_type: String, pub count: i64 }
pub struct QueueDepth { pub queue: String, pub count: i64 }
pub struct Bucket { pub start: DateTime<Utc>, pub succeeded: i64, pub failed: i64 }
pub struct JobStatsRow { pub states: Vec<StateCount>, pub types: Vec<TypeCount>, pub queues: Vec<QueueDepth>, pub throughput: Vec<Bucket> }
impl JobsRepo {
    pub async fn get(conn, id: Uuid) -> Result<Option<JobRow>, DataError>;
    pub async fn attempts(conn, id: Uuid) -> Result<Vec<AttemptRow>, DataError>;       // ordered by attempt
    pub async fn retried_by(conn, id: Uuid) -> Result<Option<Uuid>, DataError>;        // newest job with retried_from = id
    pub async fn list(conn, filter: &JobFilter, pagination: Pagination) -> Result<ResultSet<JobRow>, DataError>;
    pub async fn stats(conn, since: DateTime<Utc>, bucket_secs: i64) -> Result<JobStatsRow, DataError>;
    pub async fn purge_finished(conn, succeeded_before: DateTime<Utc>, failed_before: DateTime<Utc>) -> Result<u64, DataError>;
}
// recurring_runs.rs
pub struct LastRun { pub name: String, pub outcome: Option<String>, pub at: DateTime<Utc> }
impl RecurringRunsRepo { pub async fn last_runs(conn) -> Result<Vec<LastRun>, DataError>; }
```

`JobFilter.state` is a string so it can carry the derived `scheduled` state: `scheduled` = `state='queued' AND run_at > now()`, `queued` = `state='queued' AND run_at <= now()`, anything else matches `state` exactly. The time range applies to the state's relevant timestamp: `finished_at` for finished states, `run_at` otherwise.

- [ ] **Step 1: Write the failing tests**

Add to `server/crates/data/tests/jobs.rs`:

```rust
use postit_data::jobs::JobFilter;
use emixdb::dto::Pagination;

async fn seed(pool: &PgPool, job_type: &str, run_at_offset_secs: i64) -> Uuid {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    let job = NewJob { id, job_type, queue: "mail", payload: &payload,
        run_at: Utc::now() + Duration::seconds(run_at_offset_secs), recurring_name: None, retried_from: None };
    JobsRepo::insert(&mut conn, &job).await.unwrap_or_else(|e| unreachable!("{e}"));
    id
}

#[sqlx::test(migrations = "migrations")]
async fn scheduled_and_queued_are_split_by_run_at(pool: PgPool) {
    seed(&pool, "send_email", -5).await;
    seed(&pool, "send_email", 3600).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let page = Pagination { page: 1, page_size: 20 };
    let queued = JobsRepo::list(&mut conn, &JobFilter { state: Some("queued".into()), ..JobFilter::default() }, page)
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    let scheduled = JobsRepo::list(&mut conn, &JobFilter { state: Some("scheduled".into()), ..JobFilter::default() }, page)
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((queued.total, scheduled.total), (1, 1));
}

#[sqlx::test(migrations = "migrations")]
async fn list_filters_by_type_and_paginates(pool: PgPool) {
    for _ in 0..3 { seed(&pool, "send_email", -1).await; }
    seed(&pool, "delete_user", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let filter = JobFilter { job_type: Some("send_email".into()), ..JobFilter::default() };
    let page = JobsRepo::list(&mut conn, &filter, Pagination { page: 2, page_size: 2 })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(page.total, 3);
    assert_eq!(page.data.len(), 1);
}

#[sqlx::test(migrations = "migrations")]
async fn stats_count_states_types_queue_depth_and_throughput(pool: PgPool) {
    let done = seed(&pool, "send_email", -1).await;
    seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, done, 1, "w", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(&mut conn, done, 1,
        &Finish { outcome: AttemptOutcome::Succeeded, state: JobState::Succeeded, next_run_at: None, error: None })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    let stats = JobsRepo::stats(&mut conn, Utc::now() - Duration::hours(24), 3600)
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    let count = |s: &str| stats.states.iter().find(|c| c.state == s).map_or(0, |c| c.count);
    assert_eq!((count("queued"), count("succeeded")), (1, 1));
    assert_eq!(stats.queues.iter().find(|q| q.queue == "mail").map(|q| q.count), Some(1));
    assert_eq!(stats.throughput.iter().map(|b| b.succeeded).sum::<i64>(), 1);
}

#[sqlx::test(migrations = "migrations")]
async fn detail_returns_attempts_in_order_and_the_retry_link(pool: PgPool) {
    let id = seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    for n in 1..=2 {
        JobsRepo::begin_attempt(&mut conn, id, n, "w", None).await.unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::finish_attempt(&mut conn, id, n,
            &Finish { outcome: AttemptOutcome::Retrying, state: JobState::Failed, next_run_at: Some(Utc::now()),
                error: Some(AttemptError { kind: ErrorKind::Retry, code: None, message: None }) })
            .await.unwrap_or_else(|e| unreachable!("{e}"));
    }
    let attempts = JobsRepo::attempts(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts.iter().map(|a| a.attempt).collect::<Vec<_>>(), vec![1, 2]);
    let child = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &NewJob { id: child, job_type: "send_email", queue: "mail", payload: &payload,
        run_at: Utc::now(), recurring_name: None, retried_from: Some(id) })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(JobsRepo::retried_by(&mut conn, id).await.unwrap_or_else(|e| unreachable!("{e}")), Some(child));
    let row = JobsRepo::get(&mut conn, child).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(row.map(|r| r.retried_from), Some(Some(id)));
}

#[sqlx::test(migrations = "migrations")]
async fn purge_deletes_old_finished_jobs_and_their_attempts_only(pool: PgPool) {
    let old = seed(&pool, "send_email", -1).await;
    let live = seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, old, 1, "w", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(&mut conn, old, 1,
        &Finish { outcome: AttemptOutcome::Succeeded, state: JobState::Succeeded, next_run_at: None, error: None })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    sqlx::query("UPDATE jobs SET finished_at = now() - interval '30 days' WHERE id = $1")
        .bind(old).execute(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    let n = JobsRepo::purge_finished(&mut conn, Utc::now() - Duration::days(7), Utc::now() - Duration::days(30))
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(n, 1);
    assert!(JobsRepo::get(&mut conn, live).await.unwrap_or_else(|e| unreachable!("{e}")).is_some());
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = $1")
        .bind(old).fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts, 0);
}
```

And in `server/crates/data/tests/recurring_runs.rs` add a test that `RecurringRunsRepo::last_runs` returns the newest finished-or-not run per name (insert two manual runs for `"purge"`, finish the older with `Succeeded`, expect one `LastRun` whose `at` is the newer row's `created_at`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-data --test jobs --test recurring_runs`
Expected: FAIL to compile (new items missing).

- [ ] **Step 3: Implement the reads**

Append to `jobs.rs`. Pattern from `audit_repo.rs` (private row struct, `query_as!`, a count query, `ResultSet`). The shared filter fragment, written out in both the list and count query:

```sql
WHERE ($1::text IS NULL OR CASE
           WHEN $1 = 'scheduled' THEN j.state = 'queued' AND j.run_at > now()
           WHEN $1 = 'queued'    THEN j.state = 'queued' AND j.run_at <= now()
           ELSE j.state = $1 END)
  AND ($2::text IS NULL OR j.job_type = $2)
  AND ($3::timestamptz IS NULL OR
       CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
            THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END >= $3)
  AND ($4::timestamptz IS NULL OR
       CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
            THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END <= $4)
```

List select (newest activity first; `id` as the tie-breaker):

```sql
SELECT j.id, j.job_type, j.queue, j.payload, j.state, j.run_at, j.attempts, j.max_attempts,
       j.recurring_name, j.retried_from, j.created_at, j.started_at, j.finished_at,
       (SELECT a.error_kind FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_kind,
       (SELECT a.error_code FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_code
FROM jobs j
<WHERE above>
ORDER BY j.created_at DESC, j.id DESC
LIMIT $5 OFFSET $6
```

The row struct holds `state: String`; convert with `JobState::from_str` in `From<Row>` (an unparseable value cannot occur because of the `CHECK`; map the error to `DataError::Conflict`).

`stats` runs four queries:

```sql
-- states (scheduled split out)
SELECT CASE WHEN state = 'queued' AND run_at > now() THEN 'scheduled' ELSE state END AS "state!", COUNT(*) AS "count!"
FROM jobs GROUP BY 1
-- types
SELECT job_type, COUNT(*) AS "count!" FROM jobs GROUP BY job_type ORDER BY job_type
-- queue depth (due and waiting for a worker)
SELECT queue, COUNT(*) AS "count!" FROM jobs WHERE state = 'queued' AND run_at <= now() GROUP BY queue ORDER BY queue
-- throughput
SELECT date_bin(make_interval(secs => $2::double precision), finished_at, TIMESTAMPTZ 'epoch') AS "start!",
       COUNT(*) FILTER (WHERE outcome = 'succeeded') AS "succeeded!",
       COUNT(*) FILTER (WHERE outcome IN ('retrying','failed','killed')) AS "failed!"
FROM job_attempts WHERE finished_at >= $1 GROUP BY 1 ORDER BY 1
```

`purge_finished`: `DELETE FROM jobs WHERE (state = 'succeeded' AND finished_at < $1) OR (state IN ('dead','killed','cancelled') AND finished_at < $2)` (attempts go by cascade; a `failed` job waiting for retry is never purged).

`last_runs` in `recurring_runs.rs`:

```sql
SELECT DISTINCT ON (name) name, outcome, created_at AS "at!"
FROM job_recurring_runs ORDER BY name, created_at DESC
```

- [ ] **Step 4: Run, prepare, gates**

Run: `cargo sqlx prepare --workspace`, `cargo test -p postit-data`, then the Global gates.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A server
git commit -m "feat(data): job ledger reads, stats and retention purge

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Audit event kinds

**Files:**
- Modify: `server/crates/data/src/audit.rs`, `server/crates/api/src/dto.rs` (`AuditEventKindDto`), `web/src/features/audit/kinds.ts` (later, Task 15)
- Test: `server/crates/data/tests/audit.rs` (whichever existing test enumerates kinds), `server/crates/api/tests/openapi.rs`

**Interfaces:**
- Produces: `AuditEventKind::{JobRetried, JobCancelled, JobDeleted, JobTriggered}` with strings `job_retried`, `job_cancelled`, `job_deleted`, `job_triggered`; `ALL` becomes `[Self; 14]`.

- [ ] **Step 1: Find every place the kind list is mirrored**

Run: `grep -rn "EmailFailed\|email_failed" server/crates web/src --include=*.rs --include=*.ts --include=*.tsx -l`
Expected: lists `data/src/audit.rs`, `api/src/dto.rs` (the `AuditEventKindDto` mirror) and `web/src/features/audit/kinds.ts`. Every file found gets the four new kinds (web is done in Task 15).

- [ ] **Step 2: Write the failing test**

In `server/crates/data/tests/` add (or extend the existing audit test file) :

```rust
#[test]
fn job_audit_kinds_round_trip() {
    for (kind, text) in [
        (AuditEventKind::JobRetried, "job_retried"),
        (AuditEventKind::JobCancelled, "job_cancelled"),
        (AuditEventKind::JobDeleted, "job_deleted"),
        (AuditEventKind::JobTriggered, "job_triggered"),
    ] {
        assert_eq!(kind.as_str(), text);
        assert_eq!(text.parse::<AuditEventKind>().ok(), Some(kind));
        assert!(AuditEventKind::ALL.contains(&kind));
    }
}
```

Run: `cargo test -p postit-data job_audit_kinds` — Expected: FAIL (variants missing).

- [ ] **Step 3: Implement**

Add the four variants to the enum, to `ALL` (length 14), and to `as_str`. Mirror them in `AuditEventKindDto` (`serde(rename_all = "snake_case")` makes `JobRetried` serialize as `job_retried`; check that the `From`/mapping impls and the dto test that compares `ALL` against the DTO still compile and pass).

- [ ] **Step 4: Gates and commit**

Run: `cargo test --workspace`, then the Global gates (the `api/tests/openapi.rs` contract test may need `cargo xtask openapi` to be re-run at Task 12; if it fails now only because `api/openapi.json` differs, defer the regeneration to Task 12 by noting it, or run `cargo xtask openapi` now and commit the JSON).

```bash
git add -A server api
git commit -m "feat(data): job console audit event kinds

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Console catalog and ledger writes at enqueue

**Files:**
- Create: `server/crates/jobs/src/console_spec.rs`
- Modify: `server/crates/jobs/src/{lib,registry,queue,recurring}.rs`
- Test: `server/crates/jobs/tests/ledger.rs`, `server/crates/jobs/tests/queue.rs`

**Interfaces:**
- Produces:

```rust
// console_spec.rs, re-exported from lib.rs
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSummary(pub std::collections::BTreeMap<String, String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actions { pub retry: bool, pub cancel: bool, pub delete: bool }

#[derive(Debug, Clone)]
pub struct ConsoleSpec { /* private: id_fields, enum_fields, actions, disabled_reason, show_error_message, schedule, queue */ }
impl ConsoleSpec {
    pub fn new() -> Self;                                          // all actions on, no fields, messages off
    pub fn id_field(self, payload_key: &'static str, label: &'static str) -> Self;   // value must parse as a UUID
    pub fn enum_field(self, payload_key: &'static str, label: &'static str) -> Self; // value must match [a-z0-9_]{1,32}
    pub fn actions(self, actions: Actions) -> Self;
    pub fn disabled(self, reason: &'static str) -> Self;           // all actions off, with an explanation
    pub fn show_error_message(self) -> Self;
    pub fn recurring(self, schedule: &str) -> Self;                // cron text shown on the recurring page
    pub fn summarize(&self, payload: &serde_json::Value) -> JobSummary;
    pub fn queue(&self) -> &'static str;                           // set by ConsoleCatalog::add from J::QUEUE
}

#[derive(Debug, Clone, Default)]
pub struct ConsoleCatalog { /* HashMap<&'static str, ConsoleSpec> */ }
impl ConsoleCatalog {
    pub fn add<J: Job>(&mut self, spec: ConsoleSpec);
    pub fn get(&self, job_type: &str) -> Option<&ConsoleSpec>;
    pub fn job_types(&self) -> Vec<&'static str>;
    pub fn recurring(&self) -> Vec<(&'static str, String)>;        // (name, cron text), sorted by name
}

// registry.rs
impl JobRegistry { pub fn with_console(self, catalog: ConsoleCatalog) -> Self; pub(crate) fn console(&self) -> &ConsoleCatalog; }

// queue.rs  (crate-internal)
pub(crate) struct EnqueueMeta<'a> { pub queue: &'a str, pub recurring_name: Option<&'a str>, pub retried_from: Option<Uuid> }
// JobQueue::enqueue_raw_in(conn, job_type, payload, run_at, meta: &EnqueueMeta) -> Result<JobId, JobsError>
```

- [ ] **Step 1: Write the failing tests**

Create `server/crates/jobs/tests/ledger.rs`:

```rust
use std::sync::Arc;

use postit_core::SystemIdGenerator;
use postit_jobs::{Job, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Probe { tag: u32 }
impl Job for Probe {
    const JOB_TYPE: &'static str = "probe";
    const QUEUE: Queue = Queue::Default;
}

fn queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_writes_a_queued_ledger_row_with_the_queue_name(pool: PgPool) {
    let id = queue(&pool).enqueue(&Probe { tag: 7 }).await.unwrap_or_else(|e| unreachable!("{e}"));
    let (state, queue_name, job_type): (String, String, String) =
        sqlx::query_as("SELECT state, queue, job_type FROM jobs WHERE id = $1")
            .bind(id.0).fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((state.as_str(), queue_name.as_str(), job_type.as_str()), ("queued", "default", "probe"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_rolled_back_enqueue_leaves_no_ledger_row_and_no_outbox_row(pool: PgPool) {
    let q = queue(&pool);
    let mut tx = pool.begin().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = q.enqueue_in(&mut tx, &Probe { tag: 1 }, None).await.unwrap_or_else(|e| unreachable!("{e}"));
    tx.rollback().await.unwrap_or_else(|e| unreachable!("{e}"));
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE id = $1").bind(id.0)
        .fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1").bind(id.0)
        .fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((jobs, outbox), (0, 0));
}
```

Create `server/crates/jobs/tests/console_spec.rs` (or put in a `#[cfg(test)]` module in `console_spec.rs`):

```rust
use postit_jobs::ConsoleSpec;
use serde_json::json;

#[test]
fn a_summary_keeps_only_declared_ids_and_enum_values() {
    let spec = ConsoleSpec::new()
        .id_field("recipient", "recipient_user_id")
        .enum_field("kind", "mail_kind");
    let payload = json!({
        "recipient": "0199c3d8-0000-7000-8000-000000000001",
        "kind": "user_approved",
        "body": "free text that must never appear",
    });
    let summary = spec.summarize(&payload);
    assert_eq!(summary.0.get("recipient_user_id").map(String::as_str), Some("0199c3d8-0000-7000-8000-000000000001"));
    assert_eq!(summary.0.get("mail_kind").map(String::as_str), Some("user_approved"));
    assert_eq!(summary.0.len(), 2);
}

#[test]
fn values_that_are_not_ids_or_plain_tokens_are_dropped() {
    let spec = ConsoleSpec::new().id_field("recipient", "recipient_user_id").enum_field("kind", "mail_kind");
    let payload = json!({ "recipient": "ada@example.com", "kind": "Hello, Ada <ada@example.com>" });
    assert!(spec.summarize(&payload).0.is_empty());
}
```

Run: `cargo test -p postit-jobs --test ledger --test console_spec`
Expected: FAIL (no `jobs` rows written; `ConsoleSpec` missing).

- [ ] **Step 2: Implement `console_spec.rs`**

```rust
use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use uuid::Uuid;

use crate::job::Job;

/// ID-and-enum fields of one job, safe to show an admin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSummary(pub BTreeMap<String, String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actions {
    pub retry: bool,
    pub cancel: bool,
    pub delete: bool,
}

impl Actions {
    pub const ALL: Self = Self { retry: true, cancel: true, delete: true };
    pub const NONE: Self = Self { retry: false, cancel: false, delete: false };
}

#[derive(Debug, Clone)]
pub struct ConsoleSpec {
    id_fields: Vec<(&'static str, &'static str)>,
    enum_fields: Vec<(&'static str, &'static str)>,
    actions: Actions,
    disabled_reason: Option<&'static str>,
    show_error_message: bool,
    schedule: Option<String>,
    /// The job type's queue name; `ConsoleCatalog::add` fills it from `J::QUEUE`, so the
    /// console can enqueue a manual run without a `JobRegistry` (the `api` role has none).
    queue: &'static str,
}

impl Default for ConsoleSpec {
    fn default() -> Self { Self::new() }
}

impl ConsoleSpec {
    #[must_use]
    pub fn new() -> Self {
        Self { id_fields: Vec::new(), enum_fields: Vec::new(), actions: Actions::ALL,
               disabled_reason: None, show_error_message: false, schedule: None,
               queue: crate::job::Queue::Default.as_str() }
    }
    #[must_use]
    pub fn id_field(mut self, payload_key: &'static str, label: &'static str) -> Self {
        self.id_fields.push((payload_key, label));
        self
    }
    #[must_use]
    pub fn enum_field(mut self, payload_key: &'static str, label: &'static str) -> Self {
        self.enum_fields.push((payload_key, label));
        self
    }
    #[must_use]
    pub fn actions(mut self, actions: Actions) -> Self { self.actions = actions; self }
    /// Turns every action off and records why (domain state decides whether work happens).
    #[must_use]
    pub fn disabled(mut self, reason: &'static str) -> Self {
        self.actions = Actions::NONE;
        self.disabled_reason = Some(reason);
        self
    }
    /// Opt in to storing and showing the handler's error message. Only for job types whose
    /// errors cannot contain user content.
    #[must_use]
    pub fn show_error_message(mut self) -> Self { self.show_error_message = true; self }
    #[must_use]
    pub fn recurring(mut self, schedule: &str) -> Self { self.schedule = Some(schedule.to_owned()); self }

    #[must_use]
    pub fn allowed(&self) -> Actions { self.actions }
    #[must_use]
    pub fn disabled_reason(&self) -> Option<&'static str> { self.disabled_reason }
    #[must_use]
    pub fn shows_error_message(&self) -> bool { self.show_error_message }
    #[must_use]
    pub fn schedule(&self) -> Option<&str> { self.schedule.as_deref() }
    #[must_use]
    pub fn queue(&self) -> &'static str { self.queue }

    #[must_use]
    pub fn summarize(&self, payload: &Value) -> JobSummary {
        let mut fields = BTreeMap::new();
        for (key, label) in &self.id_fields {
            if let Some(text) = payload.get(*key).and_then(Value::as_str)
                && Uuid::parse_str(text).is_ok()
            {
                fields.insert((*label).to_owned(), text.to_owned());
            }
        }
        for (key, label) in &self.enum_fields {
            if let Some(text) = payload.get(*key).and_then(Value::as_str)
                && is_token(text)
            {
                fields.insert((*label).to_owned(), text.to_owned());
            }
        }
        JobSummary(fields)
    }
}

fn is_token(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Clone, Default)]
pub struct ConsoleCatalog {
    specs: HashMap<&'static str, ConsoleSpec>,
}

impl ConsoleCatalog {
    pub fn add<J: Job>(&mut self, mut spec: ConsoleSpec) {
        spec.queue = J::QUEUE.as_str();
        self.specs.insert(J::JOB_TYPE, spec);
    }
    #[must_use]
    pub fn get(&self, job_type: &str) -> Option<&ConsoleSpec> { self.specs.get(job_type) }
    #[must_use]
    pub fn job_types(&self) -> Vec<&'static str> {
        let mut types: Vec<_> = self.specs.keys().copied().collect();
        types.sort_unstable();
        types
    }
    /// `(name, cron text)` of every recurring job, sorted by name.
    #[must_use]
    pub fn recurring(&self) -> Vec<(&'static str, String)> {
        let mut rows: Vec<_> = self.specs.iter()
            .filter_map(|(name, spec)| spec.schedule().map(|s| (*name, s.to_owned())))
            .collect();
        rows.sort_unstable_by_key(|(name, _)| *name);
        rows
    }
}
```

In `lib.rs` add `mod console_spec;` and `pub use console_spec::{Actions, ConsoleCatalog, ConsoleSpec, JobSummary};`.

- [ ] **Step 3: Registry hook**

In `registry.rs` add a field `pub(crate) console: crate::console_spec::ConsoleCatalog` to `JobRegistry` (it derives `Default`), plus:

```rust
#[must_use]
pub fn with_console(mut self, catalog: ConsoleCatalog) -> Self {
    self.console = catalog;
    self
}

pub(crate) fn console(&self) -> &ConsoleCatalog { &self.console }
```

- [ ] **Step 4: Ledger insert in `JobQueue`**

In `queue.rs` add the `EnqueueMeta` struct from the Interfaces block and change `enqueue_raw_in` to take `meta: &EnqueueMeta<'_>`; after the outbox insert, in the same connection:

```rust
JobsRepo::insert(conn, &NewJob {
    id, job_type, queue: meta.queue, payload,
    run_at: run_at.unwrap_or_else(Utc::now),
    recurring_name: meta.recurring_name, retried_from: meta.retried_from,
}).await?;
```

(Compute `run_at` once into a local and use it for both inserts.) `enqueue_in::<J>` passes `EnqueueMeta { queue: J::QUEUE.as_str(), recurring_name: None, retried_from: None }`. In `recurring.rs`, add `pub queue: Queue` to `RecurringSpec`, set it in `register_recurring` (`queue: J::QUEUE`), and pass `EnqueueMeta { queue: spec.queue.as_str(), recurring_name: Some(name), retried_from: None }` from `claim_and_enqueue` (add a `queue: Queue` argument). Update the two other `enqueue_raw_in` callers the compiler reports.

- [ ] **Step 5: Run, gates, commit**

Run: `cargo sqlx prepare --workspace` (only if `query!` text in `jobs` crate changed; this task adds none there), `cargo test -p postit-jobs`, then the Global gates.
Expected: PASS, including the pre-existing jobs tests.

```bash
git add -A server
git commit -m "feat(jobs): console catalog and ledger rows at enqueue

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Record every try in `dispatch`

**Files:**
- Create: `server/crates/jobs/src/ledger.rs`
- Modify: `server/crates/jobs/src/{lib,dispatch,backend}.rs`
- Test: `server/crates/jobs/tests/ledger.rs`

**Interfaces:**
- Consumes: Task 2 `JobsRepo`, Task 5 `ConsoleCatalog` via `registry.console()`.
- Produces (crate-internal): `dispatch::dispatch_detailed(...) -> Dispatched` and `ledger::run_recorded(state: &HandlerState, envelope, job_id, attempt) -> Outcome`. `dispatch(...)` keeps its signature and returns `dispatch_detailed(...).outcome`.

```rust
// dispatch.rs
pub(crate) struct Failure { pub kind: ErrorKind, pub message: String }
pub(crate) struct Dispatched { pub outcome: Outcome, pub failure: Option<Failure> }
pub(crate) async fn dispatch_detailed(pool, registry, envelope, job_id, attempt) -> Dispatched;
```

- [ ] **Step 1: Write the failing tests**

Add to `server/crates/jobs/tests/ledger.rs` (uses `RunningWorker`, `wait_until` from `postit_jobs::testkit`; see `tests/worker.rs` for the registry/handler idiom):

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use postit_jobs::testkit::{RunningWorker, wait_until};
use postit_jobs::{JobContext, JobError, JobRegistry, RetryPolicy, ConsoleCatalog, ConsoleSpec};

async fn job_state(pool: &PgPool, id: uuid::Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1").bind(id).fetch_one(pool).await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_successful_job_ends_succeeded_with_one_attempt(pool: PgPool) {
    let mut registry = JobRegistry::default();
    registry.register(RetryPolicy::None, |_: Probe, _: JobContext| async { Ok(()) })
        .unwrap_or_else(|e| unreachable!("{e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = queue(&pool).enqueue(&Probe { tag: 1 }).await.unwrap_or_else(|e| unreachable!("{e}")).0;
    let p = pool.clone();
    assert!(wait_until_async(Duration::from_secs(10), move || { let p = p.clone(); async move { job_state(&p, id).await == "succeeded" } }).await);
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = $1 AND outcome = 'succeeded'")
        .bind(id).fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts, 1);
    worker.stop().await;
}
```

`wait_until` is synchronous (`Fn() -> bool`); add a small async polling helper in the test file (`wait_until_async`) that loops 50 ms until the async predicate holds, or use `tokio::time::timeout` with a loop. Further tests in the same file, one per bullet (same structure: build a registry whose handler produces the case, run a `RunningWorker`, poll the ledger):

- `a_failing_job_with_retries_left_is_failed_then_dead_when_exhausted`: `RetryPolicy::Backoff { max_attempts: Some(2), initial: 100ms, max: 100ms }`, handler always `Err(JobError::Retry("secret@example.com".into()))`. Expect final state `dead`, two attempts (`retrying`, `failed`), `error_kind` of the last is `retries_exhausted`, `error_code` `max_attempts`, and `error_message IS NULL` for both (type did not opt in).
- `a_fatal_error_is_killed`: handler `Err(JobError::Fatal("bad".into()))` → state `killed`, kind `fatal`.
- `a_panicking_handler_is_killed_with_the_panic_kind`.
- An unregistered job type never reaches `dispatch` through a worker (the relay only claims registered types, so the row waits in the outbox and the ledger shows it `queued`); its `UnknownType` classification is covered by the `dispatch_detailed` unit test below.
- `an_opted_in_type_stores_its_error_message`: `ConsoleCatalog` with `ConsoleSpec::new().show_error_message()` for `Probe`, attached with `registry.with_console(catalog)`; handler `Err(Fatal("disk full"))` → `error_message = 'disk full'`.
- `a_cancelled_job_never_runs`: enqueue `Probe` with no worker running; call `JobsRepo::cancel` on its ledger row (queued) but leave the outbox row, so the relay still pushes it (the "cancelled after relay" path); start a `RunningWorker` whose handler increments an `AtomicUsize`; wait until `postit_jobs::testkit::finished_job_ids(&pool)` contains the job ID (the storage acked it, so `run_recorded` has returned); assert the counter is 0, the state is `cancelled`, and `job_attempts` has no row for the job.
- `a_ledger_write_failure_does_not_stop_the_job`: `DROP TABLE job_attempts` before starting the worker; handler increments a counter; assert the counter reaches 1.
- `a_worker_run_job_leaves_no_open_attempt`: run a succeeding and a fatally failing `Probe` to completion; assert `SELECT COUNT(*) FROM job_attempts WHERE finished_at IS NULL` is 0. (The crash path, an open attempt closed as `interrupted` by the next try, is pinned at repository level by Task 2's `a_new_try_closes_an_open_attempt_as_interrupted`; apalis re-enqueues an abandoned task with attempts + 1, `APALIS_NOTES.md` item 9, which is exactly that call sequence.)
- `a_cancelled_recurring_run_is_recorded_failed`: register a recurring `Tick` job (`register_recurring("0 0 0 1 1 *", …)`) with a counting handler; insert a manual run with `RecurringRunsRepo::insert_manual`, enqueue it the way `trigger` will (Task 8) or via `JobQueue::enqueue_in` with a `{"run_id": …}` payload, cancel its ledger row, start the worker, wait until acked; assert the handler never ran and the run row's `outcome` is `failed` (so the Recurring page never shows a run without an outcome forever).

Add a `dispatch.rs` unit test in the existing `tests` module:

```rust
#[sqlx::test(migrations = "../data/migrations")]
async fn dispatch_detailed_classifies_each_failure(pool: PgPool) {
    let registry = registry();
    let id = JobId(uuid::Uuid::now_v7());
    let kind = |d: &Dispatched| d.failure.as_ref().map(|f| f.kind);
    assert_eq!(kind(&dispatch_detailed(&pool, &registry, envelope("nope"), id, 1).await), Some(ErrorKind::UnknownType));
    assert_eq!(kind(&dispatch_detailed(&pool, &registry, envelope("broken"), id, 1).await), Some(ErrorKind::Fatal));
    assert_eq!(kind(&dispatch_detailed(&pool, &registry, envelope("flaky"), id, 1).await), Some(ErrorKind::Retry));
    assert_eq!(kind(&dispatch_detailed(&pool, &registry, envelope("flaky"), id, 2).await), Some(ErrorKind::RetriesExhausted));
}
```

Run: `cargo test -p postit-jobs`
Expected: FAIL (ledger rows never leave `queued`; `dispatch_detailed` missing).

- [ ] **Step 2: Refactor `dispatch` into `dispatch_detailed`**

In `dispatch.rs`, add `Failure`, `Dispatched`; rename the body of `dispatch` to `dispatch_detailed` returning `Dispatched` with these mappings, and keep:

```rust
pub(crate) async fn dispatch(pool: &PgPool, registry: &JobRegistry, envelope: Envelope, job_id: JobId, attempt: u32) -> Outcome {
    dispatch_detailed(pool, registry, envelope, job_id, attempt).await.outcome
}
```

Also add, reusing the existing `recurring_run_id` extraction and `record_run_outcome` (move the extraction into a private `fn recurring_run_id(registration, &payload) -> Option<Uuid>` shared by both):

```rust
/// Closes the recurring run of a job that was cancelled before it ran, as `Failed`.
/// A no-op for non-recurring and unregistered job types.
pub(crate) async fn record_skipped_run(pool: &PgPool, registry: &JobRegistry, envelope: &Envelope) {
    let Some(run_id) = registry.get(&envelope.job_type).and_then(|r| recurring_run_id(r, &envelope.payload)) else {
        return;
    };
    if let Err(err) = record_run_outcome(pool, run_id, RunOutcome::Failed).await {
        tracing::warn!(%run_id, error = %err, "could not record recurring run outcome");
    }
}
```

Mappings: unregistered → `Abort(msg)` with `ErrorKind::UnknownType`; `Err(Fatal(m))` → `Abort(m)` with `Fatal`; panic → `Abort("the job handler panicked")` with `Panic`; `Err(Retry(m))` with `delay_before_next(attempt) == Some(d)` → `RetryAfter(d)` with `Retry`; `Retry` with `None` → `Abort(m)` with `RetriesExhausted`; `Ok` → `Done`, no failure. The recurring-run outcome bookkeeping stays unchanged, placed after the outcome is computed.

- [ ] **Step 3: Implement `ledger.rs`**

```rust
//! Records every try of every job in the job ledger (`jobs`, `job_attempts`). The ledger is
//! best-effort around execution: a failed ledger write is logged and never changes whether
//! or how a job runs.

use chrono::Utc;
use postit_data::jobs::{AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobState, JobsRepo};

use crate::backend::HandlerState;
use crate::dispatch::{self, Dispatched, Envelope, Outcome};
use crate::job::JobId;

pub(crate) async fn run_recorded(
    state: &HandlerState,
    envelope: Envelope,
    job_id: JobId,
    attempt: u32,
) -> Outcome {
    let attempt_no = i32::try_from(attempt).unwrap_or(i32::MAX);
    let job_type = envelope.job_type.clone();
    let max_attempts = state
        .registry
        .get(&job_type)
        .and_then(|r| r.retry.max_attempts())
        .and_then(|n| i32::try_from(n).ok());

    let begin = match begin(state, job_id, attempt_no, max_attempts).await {
        Ok(begin) => begin,
        Err(err) => {
            tracing::warn!(job_id = %job_id.0, error = %err, "could not record the start of a try");
            Begin::Untracked
        }
    };
    if begin == Begin::Cancelled {
        // The handler never runs. A recurring run still gets an outcome, so its
        // `job_recurring_runs` row does not stay open forever.
        dispatch::record_skipped_run(&state.pool, &state.registry, &envelope).await;
        return Outcome::Done;
    }

    let dispatched = dispatch::dispatch_detailed(&state.pool, &state.registry, envelope, job_id, attempt).await;
    if begin == Begin::Run
        && let Err(err) = finish(state, job_id, attempt_no, &job_type, &dispatched).await
    {
        tracing::warn!(job_id = %job_id.0, error = %err, "could not record the end of a try");
    }
    dispatched.outcome
}

async fn begin(state: &HandlerState, id: JobId, attempt: i32, max: Option<i32>) -> Result<Begin, postit_data::DataError> {
    let mut conn = state.pool.acquire().await?;
    JobsRepo::begin_attempt(&mut conn, id.0, attempt, &state.worker, max).await
}

async fn finish(state: &HandlerState, id: JobId, attempt: i32, job_type: &str, d: &Dispatched) -> Result<(), postit_data::DataError> {
    let show_message = state.registry.console().get(job_type).is_some_and(|s| s.shows_error_message());
    let error = d.failure.as_ref().map(|f| AttemptError {
        kind: f.kind,
        code: f.kind.default_code(),
        message: show_message.then_some(f.message.as_str()),
    });
    let (outcome, job_state, next_run_at) = match (&d.outcome, d.failure.as_ref().map(|f| f.kind)) {
        (Outcome::Done, _) => (AttemptOutcome::Succeeded, JobState::Succeeded, None),
        (Outcome::RetryAfter(delay), _) => (
            AttemptOutcome::Retrying, JobState::Failed,
            Some(Utc::now() + chrono::Duration::from_std(*delay).unwrap_or_default()),
        ),
        (Outcome::Abort(_), Some(ErrorKind::RetriesExhausted)) => (AttemptOutcome::Failed, JobState::Dead, None),
        (Outcome::Abort(_), _) => (AttemptOutcome::Killed, JobState::Killed, None),
    };
    let mut conn = state.pool.acquire().await?;
    JobsRepo::finish_attempt(&mut conn, id.0, attempt, &Finish { outcome, state: job_state, next_run_at, error }).await
}
```

`Begin` needs `PartialEq` (it derives it in Task 2). In `backend.rs`: make `HandlerState` `pub(crate)` with `pub(crate)` fields (it is private today and `ledger.rs` reads it), and give it a `pub(crate) worker: String` built once in `Backend::run` (`format!("{}:{}", hostname, std::process::id())` where `hostname` is `std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "unknown".into())`), and change `handle` to call `ledger::run_recorded(&state, envelope, job_id, attempt)` instead of `dispatch::dispatch(...)`. Add `mod ledger;` to `lib.rs`. The `Failure` text used for `RetryAfter` in `handle` is unchanged.

- [ ] **Step 4: Run, gates, commit**

Run: `cargo test -p postit-jobs`, then Global gates.
Expected: PASS. The old dispatch tests still call `dispatch(...)`.

```bash
git add -A server
git commit -m "feat(jobs): record every try of every job in the ledger

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `JobConsole` reads

**Files:**
- Create: `server/crates/jobs/src/console.rs`
- Modify: `server/crates/jobs/src/lib.rs`
- Test: `server/crates/jobs/tests/console.rs`

**Interfaces:**
- Consumes: `JobsRepo` reads (Task 3), `ConsoleCatalog` (Task 5), `RecurringRunsRepo::last_runs`.
- Produces:

```rust
#[derive(Clone)]
pub struct JobConsole { /* pool, queue: JobQueue, catalog: Arc<ConsoleCatalog> */ }

#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    #[error("job not found")] NotFound,
    #[error("action not allowed: {0}")] NotAllowed(String),
    #[error(transparent)] Data(#[from] postit_data::DataError),
    #[error(transparent)] Jobs(#[from] JobsError),
}

// JobView, AttemptView, JobDetail, StatsView, RecurringView all #[derive(Debug, Clone)]
// (the redaction test formats a JobView with `{:?}`).
pub struct JobView {            // one row, safe to serialize
    pub id: Uuid, pub job_type: String, pub queue: String, pub state: String /* incl. "scheduled" */,
    pub attempts: i32, pub max_attempts: Option<i32>, pub run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>, pub started_at: Option<DateTime<Utc>>, pub finished_at: Option<DateTime<Utc>>,
    pub recurring_name: Option<String>, pub summary: JobSummary,
    pub error_kind: Option<String>, pub error_code: Option<String>,
    pub actions: Actions, pub actions_disabled_reason: Option<String>,
}
pub struct AttemptView { pub attempt: i32, pub started_at, pub finished_at: Option<..>, pub worker: String,
    pub outcome: Option<String>, pub error_kind: Option<String>, pub error_code: Option<String>, pub error_message: Option<String> }
pub struct JobDetail { pub job: JobView, pub attempts: Vec<AttemptView>, pub retried_from: Option<Uuid>, pub retried_by: Option<Uuid> }
pub struct StatsView { pub states: Vec<(String, i64)>, pub types: Vec<(String, i64)>, pub queues: Vec<(String, i64)>, pub throughput: Vec<Bucket>, pub window_hours: u32 }
pub struct RecurringView { pub name: String, pub schedule: String, pub last_run_at: Option<DateTime<Utc>>, pub last_outcome: Option<String>, pub next_run_at: Option<DateTime<Utc>> }

impl JobConsole {
    pub fn new(pool: PgPool, queue: JobQueue, catalog: ConsoleCatalog) -> Self;
    pub async fn list(&self, filter: JobFilter, pagination: Pagination) -> Result<ResultSet<JobView>, ConsoleError>;
    pub async fn detail(&self, id: Uuid) -> Result<JobDetail, ConsoleError>;
    pub async fn stats(&self, window_hours: u32) -> Result<StatsView, ConsoleError>;
    pub async fn recurring(&self) -> Result<Vec<RecurringView>, ConsoleError>;
}
```

`actions` is computed per job from the catalog entry (unknown types: all actions off, reason `"job type is not registered in this release"`) and the job's state: `retry` for `failed|dead|killed|cancelled`, `cancel` for `queued|scheduled|failed`, `delete` for finished states; each ANDed with the spec's allowed actions. `error_message` in `AttemptView` is only what the ledger stored (null unless the type opted in). `JobView::state` maps stored `queued` with `run_at > now` to `"scheduled"`.

- [ ] **Step 1: Write the failing tests**

`server/crates/jobs/tests/console.rs`:

```rust
use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_core::SystemIdGenerator;
use postit_data::jobs::JobFilter;
use postit_jobs::{ConsoleCatalog, ConsoleSpec, Job, JobConsole, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Mail { recipient: uuid::Uuid, kind: String, body: String }
impl Job for Mail {
    const JOB_TYPE: &'static str = "send_email";
    const QUEUE: Queue = Queue::Mail;
}

fn console(pool: &PgPool) -> (JobConsole, JobQueue) {
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().id_field("recipient", "recipient_user_id").enum_field("kind", "mail_kind"));
    (JobConsole::new(pool.clone(), queue.clone(), catalog), queue)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_listed_job_shows_a_summary_and_never_the_payload(pool: PgPool) {
    let (console, queue) = console(&pool);
    queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "user_approved".into(), body: "Dear Ada <ada@example.com>".into() })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    let page = console.list(JobFilter::default(), Pagination { page: 1, page_size: 20 })
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(page.total, 1);
    let job = &page.data[0];
    assert_eq!(job.summary.0.get("mail_kind").map(String::as_str), Some("user_approved"));
    let rendered = format!("{job:?}");
    assert!(!rendered.contains("ada@example.com") && !rendered.contains("Dear Ada"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn actions_follow_state_and_the_registration(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    let detail = console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(detail.job.actions.cancel && !detail.job.actions.retry && !detail.job.actions.delete);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_disabled_type_has_no_actions_and_a_reason(pool: PgPool) {
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().disabled("the delivery decides"));
    let console = JobConsole::new(pool.clone(), queue.clone(), catalog);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    let detail = console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!detail.job.actions.cancel);
    assert_eq!(detail.job.actions_disabled_reason.as_deref(), Some("the delivery decides"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unknown_id_is_not_found(pool: PgPool) {
    let (console, _) = console(&pool);
    assert!(matches!(console.detail(uuid::Uuid::now_v7()).await, Err(postit_jobs::ConsoleError::NotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn recurring_lists_registered_schedules_with_a_next_run(pool: PgPool) {
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().recurring("0 0 3 * * *"));
    let rows = JobConsole::new(pool.clone(), queue, catalog).recurring().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(rows.len(), 1);
    assert!(rows[0].next_run_at.is_some());
    assert_eq!(rows[0].schedule, "0 0 3 * * *");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stats_report_a_scheduled_job_separately(pool: PgPool) {
    let (console, queue) = console(&pool);
    queue.enqueue_at(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() }, chrono::Utc::now() + chrono::Duration::hours(1))
        .await.unwrap_or_else(|e| unreachable!("{e}"));
    let stats = console.stats(24).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(stats.states.iter().any(|(s, n)| s == "scheduled" && *n == 1));
}
```

Run: `cargo test -p postit-jobs --test console` — Expected: FAIL (no `JobConsole`).

- [ ] **Step 2: Implement `console.rs` reads**

Keep each method small. Core pieces:

```rust
fn view(&self, row: JobRow, now: DateTime<Utc>) -> JobView {
    let spec = self.catalog.get(&row.job_type);
    let (allowed, reason) = match spec {
        Some(spec) => (spec.allowed(), spec.disabled_reason().map(str::to_owned)),
        None => (Actions::NONE, Some("job type is not registered in this release".to_owned())),
    };
    let scheduled = row.state == JobState::Queued && row.run_at > now;
    let state_name = if scheduled { "scheduled" } else { row.state.as_str() };
    let actions = Actions {
        retry: allowed.retry && matches!(row.state, JobState::Failed | JobState::Dead | JobState::Killed | JobState::Cancelled),
        cancel: allowed.cancel && matches!(row.state, JobState::Queued | JobState::Failed),
        delete: allowed.delete && row.state.is_finished(),
    };
    let summary = spec.map(|s| s.summarize(&row.payload)).unwrap_or_default();
    // …build JobView with row.last_error_kind / last_error_code, the Actions above, and `reason` only when any allowed action was turned off by the registration
}
```

`next_run_at` for recurring: parse the cron text with `cron::Schedule::from_str` and take `upcoming(Utc).next()`. `stats(window_hours)`: clamp `window_hours` to `1..=168`; `since = now - hours`; `bucket_secs = (hours as i64 * 3600 / 24).max(300)`. `list` converts `ResultSet<JobRow>` to `ResultSet<JobView>` preserving `total` and `pagination`. `detail` returns `NotFound` when `JobsRepo::get` yields `None`. Add `pub use console::{...}` to `lib.rs`. `jobs/Cargo.toml` already has `cron`, `chrono`, `emixdb` is reachable through `postit_data`; add `emixdb.workspace = true` to `[dependencies]`.

- [ ] **Step 3: Run, gates, commit**

Run: `cargo test -p postit-jobs`, Global gates.
Expected: PASS.

```bash
git add -A server
git commit -m "feat(jobs): JobConsole reads over the ledger

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `JobConsole` actions

**Files:**
- Modify: `server/crates/jobs/src/console.rs`
- Test: `server/crates/jobs/tests/console.rs`

**Interfaces:**
- Consumes: Task 7.
- Produces (all take the caller's connection, so the API can add the audit event in the same transaction):

```rust
impl JobConsole {
    /// Returns the ID of the new job.
    pub async fn retry(&self, conn: &mut PgConnection, id: Uuid) -> Result<Uuid, ConsoleError>;
    pub async fn cancel(&self, conn: &mut PgConnection, id: Uuid) -> Result<(), ConsoleError>;
    pub async fn delete(&self, conn: &mut PgConnection, id: Uuid) -> Result<(), ConsoleError>;
    /// Returns the ID of the job the manual run enqueued.
    pub async fn trigger(&self, conn: &mut PgConnection, name: &str) -> Result<Uuid, ConsoleError>;
    pub async fn job_type_of(&self, conn: &mut PgConnection, id: Uuid) -> Result<String, ConsoleError>; // for audit details
}
```

Rules: every action runs in its own transaction on `conn` (`let mut tx = conn.begin().await?; … tx.commit().await?`), which is a savepoint when the API passes its transaction and a real transaction when a test passes a plain pool connection. Without it, `SELECT … FOR UPDATE` on an autocommit connection releases the lock at once and two concurrent retries can both create a child. Every action loads the job (`NotFound` when absent), checks the catalog (`NotAllowed(reason)` when the type is unregistered or the action is off) and the state, then performs a guarded write; a guarded write that changes nothing (a concurrent action won) is `NotAllowed("job is no longer <state>")`.

- [ ] **Step 1: Write the failing tests** (in `tests/console.rs`)

```rust
use postit_data::jobs::JobsRepo;

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_marks_the_job_and_removes_its_unrelayed_outbox_row(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    console.cancel(&mut conn(&pool).await, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1").bind(id)
        .fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outbox, 0);
    let detail = console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.job.state, "cancelled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_is_refused_for_a_running_job(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    JobsRepo::begin_attempt(&mut conn(&pool).await, id, 1, "w", None).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(console.cancel(&mut conn(&pool).await, id).await, Err(postit_jobs::ConsoleError::NotAllowed(_))));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_creates_a_linked_job_and_leaves_the_original_untouched(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    sqlx::query("UPDATE jobs SET state = 'dead', finished_at = now() WHERE id = $1").bind(id)
        .execute(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    let new_id = console.retry(&mut conn(&pool).await, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_ne!(new_id, id);
    let detail = console.detail(new_id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.retried_from, Some(id));
    assert_eq!(console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}")).job.state, "dead");
    assert_eq!(console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}")).retried_by, Some(new_id));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retrying_a_failed_job_cancels_its_pending_retry(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    sqlx::query("UPDATE jobs SET state = 'failed' WHERE id = $1").bind(id).execute(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    console.retry(&mut conn(&pool).await, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}")).job.state, "cancelled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_concurrent_retries_of_one_job_make_one_new_job(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    sqlx::query("UPDATE jobs SET state = 'dead', finished_at = now() WHERE id = $1").bind(id)
        .execute(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    let (a, b) = tokio::join!(
        async { console.retry(&mut conn(&pool).await, id).await },
        async { console.retry(&mut conn(&pool).await, id).await },
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_removes_only_finished_jobs(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue.enqueue(&Mail { recipient: uuid::Uuid::now_v7(), kind: "a".into(), body: String::new() })
        .await.unwrap_or_else(|e| unreachable!("{e}")).0;
    assert!(console.delete(&mut conn(&pool).await, id).await.is_err());
    console.cancel(&mut conn(&pool).await, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    console.delete(&mut conn(&pool).await, id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(console.detail(id).await, Err(postit_jobs::ConsoleError::NotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn trigger_enqueues_a_manual_run_for_a_recurring_job(pool: PgPool) {
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().recurring("0 0 3 * * *"));
    let console = JobConsole::new(pool.clone(), queue, catalog);
    let id = console.trigger(&mut conn(&pool).await, "send_email").await.unwrap_or_else(|e| unreachable!("{e}"));
    let detail = console.detail(id).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.job.recurring_name.as_deref(), Some("send_email"));
    assert!(matches!(console.trigger(&mut conn(&pool).await, "nope").await, Err(postit_jobs::ConsoleError::NotFound)));
}
```

For the concurrency test the second `retry` must lose: implement retry's "claim" as a guarded update that marks the source as retried. Use the ledger link: `retry` first runs `SELECT … FOR UPDATE` on the source row, re-checks the state, and refuses with `NotAllowed("already retried")` when `JobsRepo::retried_by` finds an existing child. Two connections serialize on the row lock, so exactly one creates the child. `UPDATE jobs SET state = 'failed'` directly in tests is fine.

Run: `cargo test -p postit-jobs --test console` — Expected: FAIL (methods missing).

- [ ] **Step 2: Implement**

Add to `postit-data`'s `JobsRepo` a `lock_for_action(conn, id) -> Result<Option<JobRow>, DataError>` (`SELECT … FOR UPDATE`, same columns as `get`) and re-run `cargo sqlx prepare`. In `console.rs`:

```rust
pub async fn retry(&self, conn: &mut PgConnection, id: Uuid) -> Result<Uuid, ConsoleError> {
    let mut tx = conn.begin().await.map_err(postit_data::DataError::from)?;
    let row = JobsRepo::lock_for_action(&mut tx, id).await?.ok_or(ConsoleError::NotFound)?;
    self.require(&row, Action::Retry)?;                    // catalog + state check -> NotAllowed
    if JobsRepo::retried_by(&mut tx, id).await?.is_some() {
        return Err(ConsoleError::NotAllowed("job was already retried".into()));
    }
    if row.state == JobState::Failed && !JobsRepo::cancel(&mut tx, id).await? {
        return Err(ConsoleError::NotAllowed("job is no longer waiting to retry".into()));
    }
    // A recurring job's payload names its `job_recurring_runs` row, which the original
    // run already closed; the retry gets a fresh manual run instead of reusing it.
    let (run_id, payload) = match row.recurring_name.as_deref() {
        Some(name) => {
            let (run_id, payload) = new_manual_run(&mut tx, name).await?;
            (Some(run_id), payload)
        }
        None => (None, row.payload.clone()),
    };
    let meta = EnqueueMeta { queue: &row.queue, recurring_name: row.recurring_name.as_deref(), retried_from: Some(id) };
    let new_id = self.queue.enqueue_raw_in(&mut tx, &row.job_type, &payload, None, &meta).await?;
    if let Some(run_id) = run_id {
        RecurringRunsRepo::set_job_id(&mut tx, run_id, new_id.0).await?;
    }
    tx.commit().await.map_err(postit_data::DataError::from)?;
    Ok(new_id.0)
}

/// Inserts a manual `job_recurring_runs` row for `name`; returns its ID and the job payload
/// that points at it.
async fn new_manual_run(conn: &mut PgConnection, name: &str) -> Result<(Uuid, serde_json::Value), ConsoleError> {
    let run_id = Uuid::now_v7();
    RecurringRunsRepo::insert_manual(conn, run_id, name, Utc::now()).await?;
    let payload = serde_json::to_value(RecurringPayload { run_id }).map_err(JobsError::from)?;
    Ok((run_id, payload))
}
```

(`DataError` must have `From<sqlx::Error>`; it does, the repositories use `?` on sqlx calls. If `JobsError` has no `From<serde_json::Error>`, use the variant `queue.rs` uses for `serde_json::to_value`.) `trigger` uses `new_manual_run` too, so both paths build the payload one way.

Add a test for it in Step 1's list: `retrying_a_dead_recurring_job_uses_a_new_run` — enqueue via `trigger`, set the ledger row `dead`, `retry`, then assert the new job's payload `run_id` differs from the original's and `job_recurring_runs` has two rows for the name.

`cancel`: lock, `require(Cancel)`, `if !JobsRepo::cancel(..)` → `NotAllowed("job is no longer queued")`, then `JobOutboxRepo::delete(conn, id)` (no-op when the relay already moved it). `delete`: lock, `require(Delete)`, `JobsRepo::delete_finished`. `trigger(name)`: the name must be in `catalog.recurring()` (else `NotFound`); in its transaction, `let (run_id, payload) = new_manual_run(&mut tx, name).await?`; `enqueue_raw_in(&mut tx, name, &payload, None, &EnqueueMeta { queue: spec.queue(), recurring_name: Some(name), retried_from: None })` (the queue comes from the catalog, Task 5, because the `api` role has no registry); then `RecurringRunsRepo::set_job_id(&mut tx, run_id, new_id.0)` and commit. `require` is a private helper mapping `(row.state, Action)` to the same predicates used by `view`; refactor `view` to share one function `fn allowed_actions(spec: Option<&ConsoleSpec>, state: JobState) -> (Actions, Option<String>)`.

- [ ] **Step 3: Run, gates, commit**

Run: `cargo sqlx prepare --workspace`, `cargo test -p postit-jobs`, Global gates.
Expected: PASS.

```bash
git add -A server
git commit -m "feat(jobs): JobConsole retry, cancel, delete and trigger

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Console metadata for this plan's job types, and history purge

**Files:**
- Modify: `server/crates/jobs/src/maintenance.rs`, `server/crates/jobs/src/backend.rs`, `server/crates/jobs/src/apalis_sql.rs`, `server/crates/identity/src/jobs.rs`, `server/crates/mail/src/{lib,send}.rs`, `server/crates/server/src/compose.rs`
- Test: `server/crates/jobs/tests/maintenance.rs`, `server/crates/identity/tests/recurring_jobs.rs`, `server/crates/mail/tests/send_email.rs`

**Interfaces:**
- Produces:

```rust
// postit-jobs
pub fn maintenance::console(catalog: &mut ConsoleCatalog, settings: &JobsSettings);   // job_history_purge (shows messages), data_retention
// postit-identity
pub fn jobs::console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules);          // delete_user, purge_pending_users, audit_retention
// postit-mail
pub fn console(catalog: &mut ConsoleCatalog);                                          // send_email
// postit-server
pub fn compose::console_catalog(settings: &Settings) -> ConsoleCatalog;
```

- [ ] **Step 1: Write the failing tests**

In `server/crates/jobs/tests/maintenance.rs` (existing file; follow its pattern for `run_job_history_purge`):

```rust
async fn ledger_count(pool: &PgPool, id: uuid::Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_also_deletes_old_ledger_rows(pool: PgPool) {
    postit_jobs::migrate(&pool).await.unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let old = uuid::Uuid::now_v7();
    let recent = uuid::Uuid::now_v7();
    for (id, age_days) in [(old, 30_i32), (recent, 0)] {
        sqlx::query(
            "INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at)
             VALUES ($1, 'probe', 'default', '{}', 'succeeded', now(), now() - make_interval(days => $2))",
        )
        .bind(id)
        .bind(age_days)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    }
    postit_jobs::testkit::run_job_history_purge(&pool, &postit_jobs::testkit::jobs_settings()).await;
    assert_eq!((ledger_count(&pool, old).await, ledger_count(&pool, recent).await), (0, 1));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_deletes_stale_worker_rows(pool: PgPool) {
    postit_jobs::migrate(&pool).await.unwrap_or_else(|e| unreachable!("migrate: {e}"));
    for (id, age_hours) in [("stale-worker", 48_i32), ("live-worker", 0)] {
        sqlx::query(
            "INSERT INTO apalis.workers (id, worker_type, storage_name, layers, last_seen)
             VALUES ($1, 'postit::default', 'PostgresStorage', '', now() - make_interval(hours => $2))",
        )
        .bind(id)
        .bind(age_hours)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert worker: {e}"));
    }
    postit_jobs::testkit::run_job_history_purge(&pool, &postit_jobs::testkit::jobs_settings()).await;
    let left: Vec<String> = sqlx::query_scalar("SELECT id FROM apalis.workers ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("workers: {e}"));
    assert_eq!(left, vec!["live-worker".to_string()]);
}
```

Before running, confirm the `apalis.workers` column list and types against `APALIS_NOTES.md` item 10 (stale workers) and adjust the `INSERT` column values (for example if `last_seen` is a `BIGINT` epoch rather than `TIMESTAMPTZ`, insert `extract(epoch FROM now() - …)::bigint` and make `purge_stale_workers` compare the same way). The jobs settings' `history_retention` must put a 30-day-old succeeded job past its cutoff and a fresh one inside it; check `testkit::jobs_settings()` and pick ages that straddle its value.

In `server/crates/identity/tests/recurring_jobs.rs` add a test that builds a catalog via `postit_identity::jobs::console` and asserts it holds `delete_user`, `purge_pending_users` and `audit_retention`, that the two cron types report their schedules from the given `JobSchedules`, and that none of them shows error messages. In `server/crates/mail/tests/send_email.rs` assert `postit_mail::console` registers `send_email` and that summarizing `{"kind":"user_approved","recipient":"<uuid>","params":{"type":"none"}}` yields `mail_kind` and `recipient_user_id` only.

Run the three crates' tests — Expected: FAIL.

- [ ] **Step 2: Implement the catalog functions**

```rust
// jobs/src/maintenance.rs
pub fn console(catalog: &mut ConsoleCatalog, settings: &JobsSettings) {
    catalog.add::<JobHistoryPurge>(ConsoleSpec::new().show_error_message().recurring(&settings.schedules.job_history_purge));
    catalog.add::<DataRetention>(ConsoleSpec::new().recurring(&settings.schedules.data_retention));
}

// identity/src/jobs.rs
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    catalog.add::<DeleteUser>(ConsoleSpec::new().id_field("user_id", "user_id"));
    catalog.add::<PurgePendingUsers>(ConsoleSpec::new().recurring(&schedules.purge_pending_users));
    catalog.add::<AuditRetention>(ConsoleSpec::new().recurring(&schedules.audit_retention));
}

// mail/src/send.rs, re-exported from lib.rs as `postit_mail::console`
pub fn console(catalog: &mut ConsoleCatalog) {
    catalog.add::<SendEmail>(ConsoleSpec::new().id_field("recipient", "recipient_user_id").enum_field("kind", "mail_kind"));
}
```

Check `MailKind`'s serde representation (`snake_case`?) in `mail/src/outbox.rs` so `kind` matches `[a-z0-9_]{1,32}`; adjust the test payload to the real serialization. `ConsoleCatalog::add` is generic over `Job`, so `SendEmail`, `DeleteUser`, … need no extra bounds. Export `ConsoleCatalog`/`ConsoleSpec` already done in Task 5; identity and mail already depend on `postit-jobs`.

In `compose.rs`:

```rust
#[must_use]
pub fn console_catalog(settings: &Settings) -> ConsoleCatalog {
    let mut catalog = ConsoleCatalog::default();
    postit_jobs::maintenance::console(&mut catalog, &settings.jobs);
    postit_identity::jobs::console(&mut catalog, &settings.jobs.schedules);
    postit_mail::console(&mut catalog);
    catalog
}
```

and make `compose::registry` end with `Ok(registry.with_console(console_catalog(settings)))` (it currently ends `Ok(registry)`).

- [ ] **Step 3: Extend `job_history_purge`**

In `maintenance.rs::run_job_history_purge`, after the existing apalis purge, also call (same `conn`):

```rust
let ledger = JobsRepo::purge_finished(&mut conn, succeeded_before, failed_before).await.map_err(|e| JobError::Retry(e.to_string()))?;
let workers = crate::backend::purge_stale_workers(pool, chrono::Utc::now() - chrono::Duration::days(1)).await.map_err(|e| JobError::Retry(e.to_string()))?;
tracing::info!(jobs, ledger, runs, workers, "job history purged");
```

Add `purge_stale_workers` to `apalis_sql.rs` (`DELETE FROM apalis.workers WHERE last_seen < $1`, runtime query, doc-comment citing `APALIS_NOTES.md` item 6) and a thin wrapper in `backend.rs`. Keep the ledger purge tolerant: ledger and apalis purges are independent statements.

- [ ] **Step 4: Run, gates, commit**

Run: `cargo test --workspace`, Global gates.
Expected: PASS.

```bash
git add -A server
git commit -m "feat(jobs,identity,mail): console metadata and ledger retention

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: API — read endpoints, DTOs, error code, state wiring

**Files:**
- Create: `server/crates/api/src/dto_jobs.rs`, `server/crates/api/src/routes/jobs.rs`
- Modify: `server/crates/api/src/{lib,state,error,openapi,testkit}.rs`, `server/crates/api/src/routes/mod.rs`, `server/crates/server/src/lib.rs` (api_state)
- Test: `server/crates/api/tests/jobs.rs`

**Interfaces:**
- Consumes: `JobConsole` (Tasks 7–8).
- Produces: `AppState.console: JobConsole`; `ErrorCode::JobActionNotAllowed` (`job_action_not_allowed`, 409, "Job action not allowed", `ALL` becomes 18); routes `GET /admin/jobs`, `GET /admin/jobs/stats`, `GET /admin/jobs/{id}`, `GET /admin/jobs/recurring`.

DTOs (`dto_jobs.rs`, all `Serialize + ToSchema`, `snake_case`):

```rust
pub enum JobStateDto { Queued, Scheduled, Running, Succeeded, Failed, Dead, Killed, Cancelled }
pub struct JobActionsDto { pub retry: bool, pub cancel: bool, pub delete: bool }
pub struct JobDto { id: Uuid, job_type: String, queue: String, state: JobStateDto, attempts: i32, max_attempts: Option<i32>,
    run_at, created_at, started_at: Option<..>, finished_at: Option<..> (DateTime<Utc>), recurring_name: Option<String>,
    summary: BTreeMap<String,String>, error_kind: Option<String>, error_code: Option<String>,
    actions: JobActionsDto, actions_disabled_reason: Option<String> }
pub struct AttemptDto { attempt: i32, started_at, finished_at: Option<..>, worker: String, outcome: Option<String>,
    error_kind: Option<String>, error_code: Option<String>, error_message: Option<String> }
pub struct JobDetailDto { job: JobDto, attempts: Vec<AttemptDto>, retried_from: Option<Uuid>, retried_by: Option<Uuid> }
pub struct JobCountDto { key: String, count: u64 }
pub struct ThroughputBucketDto { start: DateTime<Utc>, succeeded: u64, failed: u64 }
pub struct JobStatsDto { window_hours: u32, states: Vec<JobCountDto>, types: Vec<JobCountDto>, queues: Vec<JobCountDto>, throughput: Vec<ThroughputBucketDto> }
pub struct RecurringJobDto { name: String, schedule: String, last_run_at: Option<..>, last_outcome: Option<String>, next_run_at: Option<..> }
pub struct JobActionResponse { job_id: Uuid }   // retry returns the new job's id; trigger returns the enqueued job's id
```

`state` is an enum so the web client is typed; new values are additive (clients tolerate unknown ones).

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/jobs.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use sqlx::PgPool;

/// Signs `sub` in (provisioned `pending`) and, when `approve`, has the admin set it `active`
/// the way `users.rs::approve_then_role_change_works_and_takes_effect` does.
async fn user_token(app: &TestApp, sub: &str, approve: bool) -> String {
    let token = app.token(sub);
    let me = app.call(Method::GET, "/api/v1/me", Some(&token), None).await;
    if approve {
        let id = me.body["id"].as_str().unwrap_or_default().to_string();
        let res = app.call(Method::PATCH, &format!("/api/v1/users/{id}"), Some(&app.token(ADMIN_SUB)),
            Some(serde_json::json!({ "status": "active" }))).await;
        assert_eq!(res.status, StatusCode::OK);
    }
    token
}

#[sqlx::test(migrations = "../data/migrations")]
async fn members_pending_users_and_anonymous_callers_are_refused_on_every_route(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None).await; // bootstrap admin first
    let member = user_token(&app, "mallory", true).await;
    let pending = user_token(&app, "pat", false).await;
    let id = uuid::Uuid::now_v7();
    let routes = [
        (Method::GET, "/api/v1/admin/jobs".to_string()),
        (Method::GET, "/api/v1/admin/jobs/stats".to_string()),
        (Method::GET, format!("/api/v1/admin/jobs/{id}")),
        (Method::POST, format!("/api/v1/admin/jobs/{id}/retry")),
        (Method::POST, format!("/api/v1/admin/jobs/{id}/cancel")),
        (Method::DELETE, format!("/api/v1/admin/jobs/{id}")),
        (Method::GET, "/api/v1/admin/jobs/recurring".to_string()),
        (Method::POST, "/api/v1/admin/jobs/recurring/purge_pending_users/trigger".to_string()),
    ];
    for (method, path) in routes {
        let anon = app.call(method.clone(), &path, None, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{method} {path}");
        for token in [&member, &pending] {
            let denied = app.call(method.clone(), &path, Some(token), None).await;
            assert_eq!(denied.status, StatusCode::FORBIDDEN, "{method} {path}");
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_rejects_bad_filters_with_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    for query in ["state=nope", "from=2026-02-01T00:00:00Z&to=2026-01-01T00:00:00Z", "page=0", "page_size=101", "bogus=1"] {
        let res = app.call(Method::GET, &format!("/api/v1/admin/jobs?{query}"), Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(res.body["code"], "validation_failed");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unknown_job_id_is_404(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let res = app.call(Method::GET, &format!("/api/v1/admin/jobs/{}", uuid::Uuid::now_v7()), Some(&admin), None).await;
    assert_eq!((res.status, res.body["code"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_and_paginates_per_state(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    for i in 0..3 {
        sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'send_email', 'mail', '{}', $2, now(), CASE WHEN $2 = 'succeeded' THEN now() END)")
            .bind(uuid::Uuid::now_v7()).bind(if i == 0 { "succeeded" } else { "dead" })
            .execute(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    }
    let dead = app.call(Method::GET, "/api/v1/admin/jobs?state=dead&page_size=1&page=2", Some(&admin), None).await;
    assert_eq!(dead.status, StatusCode::OK);
    assert_eq!(dead.body["total"], 2);
    assert_eq!(dead.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(dead.body["data"][0]["state"], "dead");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stats_and_recurring_return_the_documented_shapes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let stats = app.call(Method::GET, "/api/v1/admin/jobs/stats", Some(&admin), None).await;
    assert_eq!(stats.status, StatusCode::OK);
    assert!(stats.body["states"].is_array() && stats.body["throughput"].is_array());
    let recurring = app.call(Method::GET, "/api/v1/admin/jobs/recurring", Some(&admin), None).await;
    assert_eq!(recurring.status, StatusCode::OK);
    assert!(recurring.body.is_array());
}
```

`TestApp` builds its `JobConsole` with a catalog containing the types these tests use (Step 3). Run: `cargo test -p postit-api --test jobs` — Expected: FAIL to compile.

- [ ] **Step 2: Error code**

In `error.rs` add `JobActionNotAllowed` to the enum, to `ALL` (`[Self; 18]`), `as_str` → `"job_action_not_allowed"`, `status` → `StatusCode::CONFLICT` (join the existing `UserDeleting | LastAdmin | IdempotencyInProgress` arm), `title` → `"Job action not allowed"`. Fix any exhaustive-match compile errors the compiler reports (the OpenAPI enum and `error.rs` tests).

- [ ] **Step 3: State, test kit, server wiring**

`state.rs`: add `pub console: postit_jobs::JobConsole,` to `AppState`. `testkit.rs`: build `JobConsole::new(pool.clone(), jobs.clone(), catalog)` (after `jobs` is created and before `JobQueue` is moved into `MailOutbox::new(jobs)` — clone `jobs` first), where the catalog is built with `postit_mail::console(&mut c); postit_identity::jobs::console(&mut c, &schedules); postit_jobs::maintenance::console(&mut c, &postit_jobs::testkit::jobs_settings());` (`testkit` feature of jobs is already a dev-dependency; make the `api` `testkit` feature also enable `postit-jobs/testkit` if `testkit.rs` is compiled under that feature). `server/src/lib.rs::api_state`: pass `console: JobConsole::new(pool.clone(), jobs.clone(), compose::console_catalog(&settings))`.

- [ ] **Step 4: DTOs and handlers**

`dto_jobs.rs` holds the DTOs above plus `From` conversions from `JobView`, `AttemptView`, `JobDetail`, `StatsView`, `RecurringView`. Handlers in `routes/jobs.rs`, modeled on `routes/audit.rs`:

```rust
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct JobListQuery {
    /// `queued`, `scheduled`, `running`, `succeeded`, `failed`, `dead`, `killed` or `cancelled`.
    #[param(value_type = Option<JobStateDto>)]
    pub state: Option<String>,
    pub job_type: Option<String>,
    /// Inclusive lower bound (RFC 3339) on the state's relevant timestamp.
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}
```

`list`: `RequireAdmin(_)`, `PageQuery{..}.to_pagination()?`, reject `from > to` with `ValidationFailed`, validate `state` against `JobStateDto`'s snake_case names (unknown → 422), then `state.console.list(JobFilter {..}, pagination)`; `ConsoleError` → `ApiError`: `NotFound → ErrorCode::NotFound`, `NotAllowed(reason) → JobActionNotAllowed.with_detail(reason)`, anything else `ApiError::internal`. Put the mapping in one `impl From<ConsoleError> for ApiError` in `routes/jobs.rs`. `stats`: optional `window_hours` query (1–168, default 24; out of range → 422). `get`: `ApiPath<Uuid>`; `recurring`: returns `Json<Vec<RecurringJobDto>>`. Every `#[utoipa::path]` copies the response list from `audit::list` (200, 401, 403, 422 where applicable, 404 for `{id}`, 429, 500, 503) and uses `tag = "admin"`. Register the four routes in `routes/mod.rs` — **register `/admin/jobs/stats` and `/admin/jobs/recurring` before `/admin/jobs/{id}`** — and add the handlers, DTOs and `JobStateDto` to `openapi.rs`.

- [ ] **Step 5: Run, gates, commit**

Run: `cargo test -p postit-api`, Global gates. (`api/tests/openapi.rs` or the `contract` test will fail until Task 12 regenerates `api/openapi.json`; run `cargo xtask openapi` now so the committed spec matches, and commit it.)
Expected: PASS.

```bash
git add -A server api web/src/api/schema.d.ts
git commit -m "feat(api): admin job read endpoints

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: API — action endpoints, audit, exit-criteria tests

**Files:**
- Modify: `server/crates/api/src/routes/jobs.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `server/crates/api/tests/jobs.rs`

**Interfaces:**
- Produces: `POST /admin/jobs/{id}/retry` → 200 `JobActionResponse{job_id: new}`, `POST /admin/jobs/{id}/cancel` → 204, `DELETE /admin/jobs/{id}` → 204, `POST /admin/jobs/recurring/{name}/trigger` → 202 `JobActionResponse{job_id}`. All 404/409 per the spec.

- [ ] **Step 1: Write the failing tests** (append to `tests/jobs.rs`)

```rust
use postit_core::{AuditEventId, UserId};

async fn audit_count(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = $1").bind(kind)
        .fetch_one(pool).await.unwrap_or_else(|e| unreachable!("{e}"))
}

async fn insert_job(pool: &PgPool, state: &str) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'purge_pending_users', 'maintenance', '{}', $2, now(), CASE WHEN $2 IN ('succeeded','dead','killed','cancelled') THEN now() END)")
        .bind(id).bind(state).execute(pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_writes_its_audit_event_and_a_second_cancel_is_409(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "queued").await;
    let first = app.call(Method::POST, &format!("/api/v1/admin/jobs/{id}/cancel"), Some(&admin), None).await;
    assert_eq!(first.status, StatusCode::NO_CONTENT);
    assert_eq!(audit_count(&pool, "job_cancelled").await, 1);
    let second = app.call(Method::POST, &format!("/api/v1/admin/jobs/{id}/cancel"), Some(&admin), None).await;
    assert_eq!((second.status, second.body["code"].as_str()), (StatusCode::CONFLICT, Some("job_action_not_allowed")));
    assert_eq!(audit_count(&pool, "job_cancelled").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_returns_the_new_job_and_audits_both_ids(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "dead").await;
    let res = app.call(Method::POST, &format!("/api/v1/admin/jobs/{id}/retry"), Some(&admin), None).await;
    assert_eq!(res.status, StatusCode::OK);
    let new_id = res.body["job_id"].as_str().unwrap_or_default().to_string();
    assert_ne!(new_id, id.to_string());
    let details: serde_json::Value = sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'job_retried'")
        .fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(details["job_id"], id.to_string());
    assert_eq!(details["new_job_id"], new_id);
    assert_eq!(details["job_type"], "purge_pending_users");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_and_trigger_are_audited(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "succeeded").await;
    let del = app.call(Method::DELETE, &format!("/api/v1/admin/jobs/{id}"), Some(&admin), None).await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(audit_count(&pool, "job_deleted").await, 1);
    let trig = app.call(Method::POST, "/api/v1/admin/jobs/recurring/purge_pending_users/trigger", Some(&admin), None).await;
    assert_eq!(trig.status, StatusCode::ACCEPTED);
    assert_eq!(audit_count(&pool, "job_triggered").await, 1);
    let missing = app.call(Method::POST, "/api/v1/admin/jobs/recurring/nope/trigger", Some(&admin), None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}
```

Then the two exit-criteria tests, in the same file (they need a real worker, `MemoryMailer`, and a mailer that fails once):

```rust
// A mailer that fails the first `failures` sends, then records.
struct FlakyMailer { inner: postit_mail::MemoryMailer, failures: std::sync::atomic::AtomicUsize }
#[async_trait::async_trait]
impl postit_mail::Mailer for FlakyMailer {
    async fn send(&self, message: postit_mail::RenderedMessage) -> Result<(), postit_mail::MailError> {
        if self.failures.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            return Err(postit_mail::MailError::Transient("smtp down: ada@example.com".into()));
        }
        self.inner.send(message).await
    }
}
```

- `retrying_a_dead_send_email_job_delivers_the_email_once`: set up a user with a verified email and `MailLoaders` like `mail/tests/send_email.rs` (copy its `handler(...)`/`ScriptedLoader`/`user(...)` helpers into `api/tests/common/mod.rs`, adding `async-trait` and the needed dev-deps to `api/Cargo.toml`), register `postit_mail::register` with `FlakyMailer{failures: 99}` and `max_attempts = 1` (so the first try goes `dead`), run a `RunningWorker`, enqueue a `send_email`, wait until its ledger state is `dead`, flip the mailer to succeed (`failures.store(0)`), `POST …/retry` as admin, wait until the new job is `succeeded`, assert `MemoryMailer::sent().len() == 1`.
- `cancelling_a_scheduled_job_stops_it_from_running`: enqueue a `Probe`-style job via `JobQueue::enqueue_at(now + 2s)` with a counting handler registered in a `RunningWorker`; `POST …/cancel` immediately; sleep until the due time plus two poll intervals; assert the counter is 0 and the ledger state is `cancelled`.
- `trigger_now_runs_purge_pending_users`: register `postit_identity::jobs::register(...)` the way `identity/tests/recurring_jobs.rs` does, run a worker, create a `pending` user older than `pending_ttl` (insert with `created_at` in the past, `pending_ttl` from that test's settings), `POST …/recurring/purge_pending_users/trigger`, wait for the user to reach `deleting`/disappear (assert as that test does).
- `the_console_never_leaks_a_send_email_failure`: after the dead job above, fetch `GET /admin/jobs/{id}` and `GET /admin/jobs?state=dead` and assert the serialized bodies contain none of `ada@example.com`, `smtp down`, the recipient name, or an `error_message` value (`error_message` must be `null` in every attempt).

Run: `cargo test -p postit-api --test jobs` — Expected: FAIL (routes missing).

- [ ] **Step 2: Implement the handlers**

Each action handler follows the same shape (shown for cancel):

```rust
pub async fn cancel(State(state): State<AppState>, RequireAdmin(admin): RequireAdmin, ApiPath(id): ApiPath<Uuid>) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await.map_err(|e| ApiError::internal(&e))?;
    let job_type = state.console.job_type_of(&mut tx, id).await?;
    state.console.cancel(&mut tx, id).await?;
    record(&state, &mut tx, &admin, AuditEventKind::JobCancelled, id, &job_type, None).await?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn record(state: &AppState, tx: &mut PgConnection, admin: &Principal, kind: AuditEventKind,
                job_id: Uuid, job_type: &str, new_job_id: Option<Uuid>) -> Result<(), ApiError> {
    let mut event = AuditEvent::new(kind).actor(admin.user_id)
        .detail("job_id", job_id.to_string()).map_err(|e| ApiError::internal(&e))?
        .detail("job_type", job_type).map_err(|e| ApiError::internal(&e))?;
    if let Some(new_id) = new_job_id {
        event = event.detail("new_job_id", new_id.to_string()).map_err(|e| ApiError::internal(&e))?;
    }
    AuditLog::record(tx, AuditEventId::from(uuid::Uuid::now_v7()), event).await.map_err(|e| ApiError::internal(&e))?;
    Ok(())
}
```

`state.settings`/`ids`: if `AppState` exposes an `IdGenerator`, use it instead of `Uuid::now_v7()` (check `UserAdminService` for the pattern; `AppState` has none, so `Uuid::now_v7()` is acceptable here). The other three handlers differ only in the console call and status: `retry` → `Json(JobActionResponse{job_id: new})` and passes `Some(new)`; `delete` → 204; `trigger` takes `ApiPath<String>`, audits `JobTriggered` with `job_type = name` and `job_id = enqueued id`, returns 202. Because the guarded write and the audit share one transaction, a failed action (409) rolls back and writes no audit event. Register the four routes in `routes/mod.rs`:

```rust
.route("/admin/jobs/{id}/retry", post(jobs::retry))
.route("/admin/jobs/{id}/cancel", post(jobs::cancel))
.route("/admin/jobs/{id}", get(jobs::get).delete(jobs::delete))
.route("/admin/jobs/recurring/{name}/trigger", post(jobs::trigger))
```

and add the handlers and `JobActionResponse` to `openapi.rs` (`paths(...)`, `components(schemas(...))`). The `ConsoleError → ApiError` conversion already exists from Task 10.

- [ ] **Step 3: Run, gates, commit**

Run: `cargo xtask openapi` (updates `api/openapi.json` and `web/src/api/schema.d.ts`; needs Node + `pnpm install` in `web/`), `cargo test --workspace`, Global gates.
Expected: PASS.

```bash
git add -A server api web/src/api/schema.d.ts
git commit -m "feat(api): admin job actions with audit events

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Server wiring and contract check

**Files:**
- Modify: `server/crates/server/src/lib.rs` (only if Task 10 left `api_state` incomplete), `CLAUDE.md`, `!ref/plans/02. foundation.md`
- Test: `server/crates/server/tests/` (the existing start-up test), `cargo xtask openapi --check`

**Interfaces:** none new.

- [ ] **Step 1: Verify wiring end to end**

Run (from `server/`): `cargo test -p postit-server` and `cargo xtask openapi --check`.
Expected: PASS; `--check` reports the committed `api/openapi.json` and `web/src/api/schema.d.ts` are current. If the server tests start an `api` role without a registry, confirm `/api/v1/admin/jobs` answers 200 for the bootstrap admin (add a test in the server crate's existing start-up test file that calls it with the test issuer if one exists there; otherwise skip, API tests already cover the handler).

- [ ] **Step 2: Record the amendments in the docs**

In `!ref/plans/02. foundation.md`, in the `postit-jobs` section, replace the sentence "implemented over apalis-postgres's listing and metrics interfaces (the ones apalis-board uses), so apalis types stay inside this crate" with "implemented over a postit-owned job ledger (`jobs`, `job_attempts` in `postit-data`) written at enqueue and by the dispatcher, so the console never reads `apalis.*`", and add `cancelled` to the console state list (eighth state), and note `job_action_not_allowed` in the error code list of `postit-api`. In `CLAUDE.md` update the "Project state" paragraph: P8 (job console) done through the API and web (finish after Task 16).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: plan 02 P8 records the job ledger and cancelled state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Web — data layer, navigation, routes

**Files:**
- Create: `web/src/features/jobs/queries.ts`, `web/src/features/jobs/labels.ts`, `web/src/features/jobs/JobState.tsx`
- Modify: `web/src/routes.tsx`, `web/src/components/Shell.tsx`, `web/src/api/errors.ts`, `web/src/features/audit/kinds.ts`, `web/package.json`
- Test: `web/src/features/jobs/queries.test.tsx`, `web/src/components/Shell.test.tsx`, `web/src/api/errors.test.ts`

**Interfaces:**
- Consumes: generated types from `web/src/api/schema.d.ts` (`components['schemas']['JobDto']`, `JobDetailDto`, `JobStatsDto`, `RecurringJobDto`, `JobStateDto`).
- Produces (`queries.ts`):

```ts
export type Job = components['schemas']['JobDto']
export type JobDetail = components['schemas']['JobDetailDto']
export type JobStats = components['schemas']['JobStatsDto']
export type RecurringJob = components['schemas']['RecurringJobDto']
export type JobState = Job['state']
export interface JobFilters { state?: JobState; jobType?: string; from?: string; to?: string; page: number; pageSize?: number }
export function useJobs(filters: JobFilters, opts?: { refetchInterval?: number })
export function useJob(id: string)
export function useJobStats(enabled?: boolean)               // polls every 5 s; paused when the tab is hidden
export function useFailedJobCount(enabled: boolean)          // failed + dead + killed from stats
export function useRecurringJobs()
export function useJobActions()                              // retry (returns new id), cancel, remove, trigger
```

- [ ] **Step 1: Add the dependency (PowerShell, from `web/`)**

```powershell
pnpm add recharts
pnpm list recharts --depth 0
```

Expected: `recharts` added with an exact version pin in `package.json` (the repo pins exact versions: if pnpm writes `^`, edit `package.json` to the resolved version and run `pnpm install`). Check the added gzipped size claim: `pnpm build` and compare `dist` size with the previous build; if the chunk growth exceeds ~150 KB gzipped, note it in the commit message and lazy-load the dashboard route (`React.lazy`) in Task 14.

- [ ] **Step 2: Write the failing tests**

`queries.test.tsx` (use `web/src/test/server.ts` MSW setup and `render.tsx` helpers, mirroring `UsersPage.test.tsx`): assert that `useFailedJobCount` returns `failed + dead + killed` from a stats response `{ states: [{key:'failed',count:2},{key:'dead',count:1},{key:'killed',count:3},{key:'queued',count:9}], … }` (expect 6), and that `useJobActions().retry` calls `POST /api/v1/admin/jobs/{id}/retry` and resolves to the new job id, invalidating `['jobs']`. In `Shell.test.tsx` add: an admin sees a "Jobs" item linking to `/jobs` with a badge showing the failed count; a member does not see "Jobs". In `errors.test.ts` add: problem code `job_action_not_allowed` maps to a specific message (`"That job can't take this action right now. It may have changed; refresh and try again."`).

Run (PowerShell): `pnpm test` — Expected: FAIL.

- [ ] **Step 3: Implement**

`queries.ts` follows `users/queries.ts` exactly (`useApi`, `unwrap`, `ensureOk`, `keepPreviousData`). Key points:

```ts
const POLL = 5_000
function visibleInterval() {
  return typeof document !== 'undefined' && document.visibilityState === 'hidden' ? false : POLL
}

export function useJobStats(enabled = true) {
  const api = useApi()
  return useQuery({
    queryKey: ['jobs', 'stats'],
    queryFn: () => unwrap(api.GET('/api/v1/admin/jobs/stats', { params: { query: { window_hours: 24 } } })),
    enabled,
    refetchInterval: visibleInterval,
    refetchIntervalInBackground: false,
  })
}

export function useFailedJobCount(enabled: boolean) {
  const stats = useJobStats(enabled)
  const n = (key: string) => stats.data?.states.find((s) => s.key === key)?.count ?? 0
  return { ...stats, data: stats.data ? n('failed') + n('dead') + n('killed') : undefined }
}

export function useJobActions() {
  const api = useApi()
  const qc = useQueryClient()
  const done = () => {
    void qc.invalidateQueries({ queryKey: ['jobs'] })
    void qc.invalidateQueries({ queryKey: ['audit'] })
  }
  return {
    retry: useMutation({
      mutationFn: async (id: string) =>
        (await unwrap(api.POST('/api/v1/admin/jobs/{id}/retry', { params: { path: { id } } }))).job_id,
      onSuccess: done,
    }),
    cancel: useMutation({ mutationFn: (id: string) => ensureOk(api.POST('/api/v1/admin/jobs/{id}/cancel', { params: { path: { id } } })), onSuccess: done }),
    remove: useMutation({ mutationFn: (id: string) => ensureOk(api.DELETE('/api/v1/admin/jobs/{id}', { params: { path: { id } } })), onSuccess: done }),
    trigger: useMutation({
      mutationFn: async (name: string) =>
        (await unwrap(api.POST('/api/v1/admin/jobs/recurring/{name}/trigger', { params: { path: { name } } }))).job_id,
      onSuccess: done,
    }),
  }
}
```

(`refetchInterval` accepts a function returning `false | number`; if the installed TanStack version's signature differs, use `(query) => …` accordingly.) `labels.ts` holds `STATE_LABELS: Record<JobState, string>` (sentence-case names), `STATE_ORDER`, and `labelFor(state: string)` returning the raw string for unknown values; `JobState.tsx` is a `Badge` wrapper coloring by state using the P7.1 tokens (`--warn-*` for failed/dead/killed, `--pink-soft` for running, `--muted` otherwise) and rendering `labelFor(state)`. Shell: add under the Admin group, after Audit,

```tsx
<NavItem to="/jobs" Icon={ListChecks}>
  <span className="flex-1">Jobs</span>
  {!!failed.data && <span className="font-mono text-xs text-faint">{failed.data}</span>}
</NavItem>
```

with `const failed = useFailedJobCount(isAdmin)` in `Nav` and `ListChecks` from `lucide-react`. `routes.tsx`: inside the `RequireAdmin` children add `{ path: 'jobs', element: <JobsDashboard /> }`, `{ path: 'jobs/list', element: <JobsList /> }`, `{ path: 'jobs/recurring', element: <RecurringPage /> }`, `{ path: 'jobs/:id', element: <JobDetailPage /> }` (the pages are stubs returning an `<h1>` until Tasks 14–16; keep each in its own file so later tasks only fill them). Add the four new kinds to `audit/kinds.ts` (`AUDIT_KINDS`, `KIND_LABELS`: "Job retried", "Job cancelled", "Job deleted", "Job triggered") mirroring the generated type. `errors.ts`: add the `job_action_not_allowed` message next to the existing code map.

- [ ] **Step 4: Run gates and commit** (PowerShell, `web/`)

```powershell
pnpm typecheck; pnpm lint; pnpm format:check; pnpm test
```

Expected: all clean/PASS. Fix formatting with `pnpm format`.

```bash
git add -A web
git commit -m "feat(web): jobs data layer, navigation and routes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Web — dashboard

**Files:**
- Create/replace: `web/src/features/jobs/JobsDashboard.tsx`, `web/src/features/jobs/ThroughputChart.tsx`, `web/src/features/jobs/JobsNav.tsx`
- Test: `web/src/features/jobs/JobsDashboard.test.tsx`

**Interfaces:**
- Consumes: `useJobStats`, `STATE_ORDER`, `labelFor`, `Panel`, `PanelTitle`.
- Produces: `/jobs` page with a sub-navigation component `JobsNav` (links: Dashboard `/jobs`, Jobs `/jobs/list`, Recurring `/jobs/recurring`) reused by Tasks 15–16.

- [ ] **Step 1: Write the failing tests**

```tsx
// JobsDashboard.test.tsx — MSW returns a stats fixture
it('shows a counter per state and the queue depth', async () => {
  server.use(http.get('*/api/v1/admin/jobs/stats', () => HttpResponse.json(statsFixture)))
  renderWithProviders(<JobsDashboard />, { route: '/jobs', admin: true })
  expect(await screen.findByRole('link', { name: /dead/i })).toHaveTextContent('1')   // each counter links to /jobs/list?state=dead
  expect(screen.getByText(/mail/i)).toBeInTheDocument()                               // queue depth row
})

it('renders the throughput chart with a text alternative', async () => {
  server.use(http.get('*/api/v1/admin/jobs/stats', () => HttpResponse.json(statsFixture)))
  renderWithProviders(<JobsDashboard />, { route: '/jobs', admin: true })
  expect(await screen.findByRole('img', { name: /jobs completed per hour/i })).toBeInTheDocument()
  expect(screen.getByRole('table', { name: /throughput data/i })).toBeInTheDocument()  // visually hidden
})

it('shows an error message when stats fail to load', async () => {
  server.use(http.get('*/api/v1/admin/jobs/stats', () => HttpResponse.json({ code: 'internal' }, { status: 500 })))
  renderWithProviders(<JobsDashboard />, { route: '/jobs', admin: true })
  expect(await screen.findByRole('alert')).toBeInTheDocument()
})
```

Use the same render helper and MSW idiom as `UsersPage.test.tsx` (look at its imports and `src/test/render.tsx`; adapt the `renderWithProviders` name to the real export). `recharts`' `ResponsiveContainer` needs a measured size in jsdom: in `src/test/setup.ts` add a `ResizeObserver` stub if it is not there, and give `ThroughputChart` a fixed `height`; assert only on the accessible wrapper and the data table, not on SVG internals.

Run: `pnpm test` — Expected: FAIL.

- [ ] **Step 2: Implement**

`JobsDashboard`: heading "Jobs" (same heading classes as Audit), `JobsNav`, then three `Panel`s: counters (a responsive grid of links, one per `STATE_ORDER` entry, large figure in `font-display`, label in mono; each links to `/jobs/list?state=<state>`), the chart panel, and queue depth (`PanelTitle` "Queue depth", rows `queue — count`). Show `messageFor(stats.error)` in a `role="alert"` paragraph on error and skeletons (`Skeleton` from `components/ui`) while loading. `ThroughputChart` wraps `recharts` `BarChart` (stacked `Bar`s `succeeded` and `failed`, `XAxis` with `tickFormatter` hour labels, `Tooltip`, `ResponsiveContainer`) colored with CSS variables (`var(--pink)` for succeeded, `var(--warn-icon)` for failed — check the exact variable names used in `index.css` and `components/Shell.tsx`, e.g. `bg-warn-icon`), wrapped in `<div role="img" aria-label="Jobs completed per hour, succeeded and failed">` and followed by a `<table className="sr-only" aria-label="Throughput data">` listing each bucket (start time, succeeded, failed). Respect reduced motion by passing `isAnimationActive={false}` when `window.matchMedia('(prefers-reduced-motion: reduce)').matches`. If Task 13 found the bundle growth large, load `ThroughputChart` via `React.lazy` with a `Suspense` fallback `Skeleton`.

- [ ] **Step 3: Gates and commit** (PowerShell, `web/`)

```powershell
pnpm typecheck; pnpm lint; pnpm format:check; pnpm test; pnpm build
```

Expected: PASS and a successful build. Then visually check in the browser (`pnpm dev`, https://postit.local:44315) against both themes at 360 px and desktop width (P7.1 exit criteria): counters readable, chart colors meet contrast in dark and light.

```bash
git add -A web
git commit -m "feat(web): jobs dashboard with throughput chart

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Web — job list and job detail with actions

**Files:**
- Create/replace: `web/src/features/jobs/JobsList.tsx`, `web/src/features/jobs/JobDetailPage.tsx`, `web/src/features/jobs/JobActions.tsx`, `web/src/features/jobs/AttemptList.tsx`
- Test: `web/src/features/jobs/JobsList.test.tsx`, `web/src/features/jobs/JobDetailPage.test.tsx`

**Interfaces:**
- Consumes: `useJobs`, `useJob`, `useJobActions`, `ConfirmDialog`, `Pager`, `JobsNav`, `JobStateBadge`.
- Produces: `/jobs/list` (state tabs, job-type select, from/to inputs, pager; URL search params as in `AuditPage`), `/jobs/:id`, and `JobActions` (renders retry/cancel/delete buttons from `job.actions`).

- [ ] **Step 1: Write the failing tests**

`JobsList.test.tsx`: renders tabs for each state; selecting "Dead" requests `?state=dead` (assert via the MSW handler reading `request.url`); typing a `from` date sends an RFC 3339 `from`; paging sends `page=2`; rows link to `/jobs/<id>`; an empty result shows "No jobs.". `JobDetailPage.test.tsx`: shows summary IDs, the retry chain links, every attempt row (outcome, worker, error kind/code), and the error message only when the API provided one; buttons follow `actions` (dead job with `retry` + `delete` shows those two, not Cancel; a job with all actions off shows `actions_disabled_reason` text); clicking Retry opens the confirm dialog, confirming posts `/retry` and navigates to `/jobs/<new id>`; a 409 `job_action_not_allowed` response shows the mapped message in a `role="alert"` and keeps the page.

Run: `pnpm test` — Expected: FAIL.

- [ ] **Step 2: Implement**

`JobsList`: copy `AuditPage`'s structure (search params as the single source of truth, `set(key, value)` resets `page`). State tabs as a `role="tablist"` of links/buttons ("All" plus each `STATE_ORDER` entry; `aria-selected` on the active). Table via `components/ui/table` with columns: State (badge), Type, Summary (comma-joined `key: value`), Attempts (`n/max`), Time (`run_at` for queued/scheduled, `finished_at` otherwise, formatted with `lib/format`), linking the row to the detail. `JobDetailPage`: `Panel` with the facts; `AttemptList` as a table (attempt, started, duration, worker, outcome, error kind/code, message when non-null). `JobActions`:

```tsx
export function JobActions({ job }: { job: Job }) {
  const { retry, cancel, remove } = useJobActions()
  const navigate = useNavigate()
  const [pending, setPending] = useState<null | 'retry' | 'cancel' | 'delete'>(null)
  // buttons rendered only when job.actions[x] is true; the matching ConfirmDialog
  // (destructive for cancel and delete) calls the mutation, then:
  //   retry  -> navigate(`/jobs/${newId}`)
  //   delete -> navigate('/jobs/list')
  // errors render messageFor(error) in <p role="alert">
  // when no action is allowed and job.actions_disabled_reason is set, show that text instead
}
```

Dialog copy: retry — "Retry this job? A new job with the same details is queued; this one stays as history."; cancel — "Cancel this job? It will not run."; delete — "Delete this job and its history? This can't be undone." Button targets ≥ 44 px (reuse `Button`), focus-visible styles come from the shared components.

- [ ] **Step 3: Gates and commit**

Run (PowerShell, `web/`): `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test`
Expected: PASS.

```bash
git add -A web
git commit -m "feat(web): job list and detail with retry, cancel and delete

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: Web — recurring jobs page, accessibility pass, docs

**Files:**
- Create/replace: `web/src/features/jobs/RecurringPage.tsx`
- Test: `web/src/features/jobs/RecurringPage.test.tsx`, `web/src/features/jobs/RouteGuard.test.tsx`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: `useRecurringJobs`, `useJobActions().trigger`, `ConfirmDialog`, `JobsNav`.

- [ ] **Step 1: Write the failing tests**

`RecurringPage.test.tsx`: lists name, cron text, last run with outcome badge ("—" when never), next run; "Trigger now" opens a confirm dialog, confirming posts `/recurring/<name>/trigger` and shows a "Triggered" status line linking to `/jobs/<job_id>`; a 404 shows the error alert. `RouteGuard.test.tsx`: a `member` visiting `/jobs`, `/jobs/list`, `/jobs/recurring` and `/jobs/<id>` is redirected exactly as `/users` and `/audit` are today (copy the assertion from the existing Gate/RequireAdmin tests), and the Jobs nav item is absent for members.

Run: `pnpm test` — Expected: FAIL.

- [ ] **Step 2: Implement**

`RecurringPage`: table with columns Name (mono), Schedule (mono), Last run (`last_run_at` + outcome badge), Next run, action cell with a "Trigger now" button (≥ 44 px, label includes the job name via `aria-label={`Trigger ${name} now`}`). Reuse `ConfirmDialog` ("Run <name> now? It runs once in the background."). On success show `<p role="status">Triggered. <Link to={`/jobs/${id}`}>View job</Link></p>`.

- [ ] **Step 3: Whole-feature checks**

PowerShell, `web/`: `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test; pnpm build`. Then, in the browser against the dev stack (`./stack.sh up development --app` or native `cargo run` + `pnpm dev`), verify the P7.1 quality bar on every Jobs screen: both themes, 360 px width, keyboard-only operation of tabs, filters, dialogs and actions, visible focus, `prefers-reduced-motion`, browser console free of CSP violations.

- [ ] **Step 4: Update `CLAUDE.md`**

Change the "Project state" paragraph to say plan 02 is implemented through P8 (job console: `JobConsole` over the `jobs`/`job_attempts` ledger, `/admin/jobs*` API, web Jobs screens) and that P9 onward is not done.

- [ ] **Step 5: Commit**

```bash
git add -A web CLAUDE.md
git commit -m "feat(web): recurring jobs page and route guard tests

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 17: Final verification (exit criteria)

**Files:** none (verification only; fix what it finds in the owning task's files).

- [ ] **Step 1: Full gates**

From `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo sqlx prepare --check --workspace`, `cargo xtask openapi --check`. From `web/` (PowerShell): `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test; pnpm build`.
Expected: all clean.

- [ ] **Step 2: Exit-criteria map**

Confirm each plan 02 P8 exit item has a passing test and name it in the final report: filtering and pagination per state → `list_filters_and_paginates_per_state`; retry of a dead `send_email` delivers once → `retrying_a_dead_send_email_job_delivers_the_email_once`; cancel stops a scheduled job → `cancelling_a_scheduled_job_stops_it_from_running`; trigger-now → `trigger_now_runs_purge_pending_users`; audit events → `cancel_writes_its_audit_event…`, `retry_returns_the_new_job…`, `delete_and_trigger_are_audited`; members and pending users get 403 → `members_pending_users_and_anonymous_callers_are_refused_on_every_route`; redaction → `the_console_never_leaks_a_send_email_failure`.

- [ ] **Step 3: Dev-stack demo (manual, record the result in the final report)**

1. `./stack.sh up development --app` (database was reset in Task 1), start the local SMTP tool (Papercut), sign in as `admin@postit.com` and `member@postit.com` (second browser profile) at `https://postit.local:44315`.
2. Stop the SMTP tool, then let `member@postit.com` sign in so a `user_pending_approval` `send_email` job is enqueued; wait for it to fail (Jobs → Jobs list, state Failed, later Dead after the retry budget — lower `mail.send_email_max_attempts` in `server/config/local.toml` to `1` for a fast demo).
3. Restart the SMTP tool. In the web app open the job, click Retry, confirm; the page moves to the new job and it reaches Succeeded; the email arrives once in the SMTP tool; Audit shows "Job retried".

- [ ] **Step 4: Final report**

Report what shipped, the amendments recorded (error codes, `cancelled`, retry of `failed`, catalog), test names for each exit item, the demo outcome, and any gap found.

---

## Self-review (done while writing)

**Spec coverage**
- Ledger tables, repositories, retention → Tasks 2, 3, 9. Migration consolidation → Task 1.
- Enqueue-time ledger + outbox visibility → Task 5. Dispatch recording, `dead`/`killed`, interrupted, cancel-after-relay → Task 6.
- `ConsoleSpec`/`JobSummary`/disabled actions/opt-in messages → Tasks 5, 9. Summaries for the six types → Task 9.
- `JobConsole` reads/actions/recurring/trigger → Tasks 7, 8. Stale `apalis.workers` purge → Task 9.
- Audit kinds → Task 4; action audit in the same transaction → Task 11.
- Eight endpoints, DTOs, `job_action_not_allowed`, 403/404/409/422 → Tasks 10, 11. Contract regeneration → Tasks 10–12.
- Web: dashboard + `recharts`, list, detail with attempts, recurring, nav badge, error mapping, guards → Tasks 13–16. Exit criteria and demo → Task 17.

**Placeholders:** the few places that say "mirror X" point at a concrete existing file and pattern (`AuditPage`, `UsersPage.test.tsx`, `mail/tests/send_email.rs`, `identity/tests/recurring_jobs.rs`); these are existing test helpers to copy, not undefined work.

**Type consistency checked:** `JobState` (data) vs `JobStateDto` (api) differ only by the derived `Scheduled`; `Begin` derives `PartialEq` in Task 2 and is compared in Task 6; `EnqueueMeta` is defined in Task 5 and used by Tasks 5 and 8; `Actions` is defined in Task 5 and used by Tasks 7–8; `ConsoleCatalog::add::<J>` records the queue in `ConsoleSpec` (Task 5), read by `trigger` and `retry` (Task 8); `JobConsole::new(pool, queue, catalog)` is used identically in Tasks 7, 10 and `compose`.

**Known judgment calls for the implementer:** `MailKind` serde form (read `mail/src/outbox.rs`); the `apalis.workers` column types (Task 9); exact CSS variable names for chart colors (read `web/src/index.css`); exact helper name exported by `web/src/test/render.tsx`.
