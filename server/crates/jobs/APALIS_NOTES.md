# apalis notes (pinned: apalis =1.0.0-rc.10, apalis-postgres =1.0.0-rc.9)

Resolved alongside: `apalis-core 1.0.0-rc.10`, `apalis-codec 0.1.0-rc.10`, `ulid 3.0.0`,
**`sqlx 0.9.0`**, which is also the workspace's own sqlx (Task 1b: the workspace moved from
sqlx 0.8 to 0.9 so there is exactly one sqlx in the dependency graph;
`cargo tree -i sqlx` shows a single `sqlx v0.9.0` used by `postit-data`, `postit-identity`,
`postit-jobs`, and `apalis-postgres`. **Observed.**).

Evidence: the Task 1 probe tests (`src/apalis_probe.rs`, removed in Task 6 once the real
backend and its tests covered the same ground; see git history), plus Task 6's tests,
marked **observed** below, and
the crate sources under `~/.cargo/registry/src/index.crates.io-*/apalis-{core,postgres}-*`,
marked **source**. Paths below are public paths unless noted.

## Read first: the pins share one sqlx with the workspace

**A. apalis-postgres rc.9 is built on sqlx 0.9, and so is the whole workspace (Task 1b).**
There is one `sqlx::PgPool` type in the dependency graph. Consequences:

- `postit_data::Db::pool()` is the same `sqlx::PgPool` type apalis-postgres's
  `PostgresStorage` and `apalis_postgres::queries::*` take, so it can be passed directly.
  No second pool, and no building `apalis_postgres::PgConnectOptions` outside
  `postit-data`.
- `postit_jobs::migrate(pool: &sqlx::PgPool)` can take the workspace pool directly and call
  `PostgresStorage::setup(pool)` on it, matching the plan's stated signature.
- **A push can run inside a caller's own transaction**, because the caller's transaction
  and apalis's queries are now on the same sqlx version:
  `apalis_postgres::queries::push_tasks(&mut *tx, queue, tasks)` accepts a
  `sqlx::Transaction<'_, Postgres>` borrowed as `&mut PgConnection` from the workspace pool.
  The relay design (task-ID dedupe, item 12) can push and delete the outbox row in the same
  transaction instead of two separate connections; see the updated item 12 below.
- In `#[sqlx::test]` tests, `pool: PgPool` (the per-test pool sqlx-test builds from
  `DATABASE_URL`) is passed straight to `postit_jobs::migrate` and to
  `apalis_postgres::PostgresStorage`/`queries::*` — no second pool construction.
- Raw SQL against `apalis.*` tables uses the same workspace pool. The probe reads
  `apalis.jobs` that way.

**B. apalis-postgres task IDs must be ULIDs.** `BackendConfig::Id = ulid::Ulid`.
`PgTaskRow::try_into` (`src/from_row.rs`) decodes `apalis.jobs.id` with
`Ulid::from_string` and fails with `Error::TaskIdError` otherwise. A hyphenated UUID string
would be stored fine but break every later fetch. A UUID v7 maps losslessly:
`ulid::Ulid::from(uuid)` / `uuid::Uuid::from(ulid)`, via ulid's `uuid` feature, which
apalis-core enables. The ULID keeps the v7 millisecond prefix. **Observed:** round-trip
equality, and the handler's `TaskId` parses back to the original UUID. This is why
`postit-jobs` depends on `ulid = "3"` (workspace dep, `features = ["uuid"]`).

## Answers

1. **Migrations.** `apalis_postgres::PostgresStorage::setup(pool: &sqlx::PgPool)
   -> Result<(), apalis_postgres::Error>` (impl on `PostgresStorage<()>`, feature
   `migrate`, on by default) runs on the workspace pool directly: `postit_jobs::migrate(pool:
   &sqlx::PgPool)` is exactly `PostgresStorage::setup(pool)`.
   `PostgresStorage::migrations() -> sqlx::migrate::Migrator` gives the migrator without
   running it. Schema: **`apalis`**, holding tables `apalis.jobs`
   and `apalis.workers` and functions `apalis.get_jobs`, `apalis.push_job`,
   `apalis.generate_ulid`, and `apalis.notify_new_jobs`, plus trigger `notify_workers`.
   History table: **`apalis._sqlx_migrations`**, set by the crate's `sqlx.toml`, so it is
   separate from `postit-data`'s `public._sqlx_migrations`. **Observed:** it exists after
   `setup`. Idempotent and concurrency-safe: the sqlx `Migrator::run_direct` takes
   `pg_advisory_lock(generate_lock_id(current_database()))` before creating the schema or
   history table. That is the same key `postit-data`'s migrator uses (same sqlx, same pool),
   so it also serialises with `postit-data`'s migrations. **Observed:** two concurrent
   `setup` calls (`tokio::join!`) plus a third call all succeed. Note: a migration runs
   `CREATE EXTENSION IF NOT EXISTS hstore`. hstore is a trusted extension (PG13+), so the
   database owner can create it, but the role running `setup` needs `CREATE` on the
   database.

2. **Storage with LISTEN/NOTIFY fetch.**
   `PostgresStorage::<Envelope>::new(&pool).with_config(apalis_postgres::Config::default().queue("postit::mail")).with_pubsub()`
   has type
   `apalis_core::backend::ext::poll_strategy::PollWith<PostgresStorage<Envelope>, StreamStrategy<apalis_postgres::Pubsub>>`.
   `with_pubsub()` opens its own `PgListener` connection per storage on channel
   `apalis::job::insert`. The trigger payload is `{job_type, id, run_at}`, and notifications
   are filtered to this queue. Always set `Config::queue`, because `PostgresStorage::new`
   defaults it to `std::any::type_name::<Args>()`. The queue name is stored in the
   `apalis.jobs.job_type` column. Useful `Config` builders: `.batch_size(n)` (default 10,
   the rows claimed per fetch), `.heartbeat_interval(d)` (default 30 s),
   `.missed_heartbeats(n)` (default 10 via `Default`), `.lock_tasks(bool)`, and
   `.persist_results(bool)`. **Observed:** a push wakes the worker immediately.
   Shared-listener alternative: `apalis_postgres::factory::PostgresStorageFactory::new(pool)`
   plus `BackendFactory::create_with_config(&mut factory, config)`, which gives one
   `PgListener` for all queues. **Source:** its listener task `unwrap()`s the connect/listen
   result, so it panics on a connection failure. Prefer `with_pubsub()` over the factory.
   **Built (Task 6):** neither. `with_pubsub()` holds one pool connection per queue (three,
   plus the outbox relay's listener, is four of a `#[sqlx::test]` pool's five), and
   the factory panics. `backend.rs` runs one `PgListener` on `apalis::job::insert` for the
   whole process (`forward_inserts`, reconnecting on loss) that routes each notification by
   `job_type` to a per-queue `futures::channel::mpsc::channel(1)`. Each queue's storage is
   `PollWith::new(storage, Strategy::new().interval(outbox_poll_interval).stream(rx))`.
   The interval picks up scheduled tasks and deferred retries within one poll interval
   (their `run_at` is in the future at insert, so no notification fires), instead of
   waiting for the 30 s heartbeat. It is listed first because `Strategy::poll_drive` stops
   at the first ready source, and a closed stream is always ready. **Source:**
   `Persisted::poll_next` fetches on every poll in state `Ready`; the strategy only arranges
   the wake-ups.

3. **Push with a caller-chosen task ID: supported.**
   `apalis::prelude::TaskBuilder::new(envelope).task_id(TaskId::from_ulid(Ulid::from(outbox_uuid))).build()`,
   then `apalis::prelude::TaskSink::push_task(&mut storage, task).await`. The error is
   `TaskSinkError<apalis_postgres::Error>`. Other useful builders: `.max_attempts(n)`,
   `.priority(n)`, `.idempotency_key(s)`, and `.run_at_*` (item 6). **Observed.**

4. **Duplicate task ID on push: an error. It never upserts.** The error is
   `TaskSinkError::PushError(apalis_postgres::Error::Database(sqlx::Error::Database(db)))`,
   with `db.code() == Some("23505")` and `db.is_unique_violation() == true`.
   `db.constraint()` names either of the two unique indexes on `apalis.jobs.id`: `unique_job_id` (from the first
   migration; **observed**) or `jobs_pkey` (added later). Match either, or match any unique
   violation that is not `idx_jobs_idempotency_key`. The row count stays 1 (**observed**).
   The sink drops a failed batch and does not keep it: a later push on the same storage
   value succeeds (**observed**; **source:** `Persisted::poll_flush` takes the buffer with
   `mem::take`). A multi-task push (`push_all`/`push_bulk`/`push_tasks`) is a single
   `INSERT … SELECT unnest(…)`, so one duplicate fails the whole batch. The relay must push
   **one task per call**.
   The other unique index is `idx_jobs_idempotency_key` on `(job_type, idempotency_key)`.
   A duplicate key fails the same way with that constraint name (**observed**). It is
   unused by the chosen design.

5. **Push inside a caller's transaction: supported, and it can be the caller's own
   transaction.** `apalis_postgres::queries::push_tasks(conn: &mut E, queue: &str, tasks:
   Vec<apalis_postgres::PgTask>)` where `for<'e> &'e mut E: sqlx::Executor<'e, Database =
   sqlx::Postgres> + Send`. `PgTask = Task<Vec<u8>>`, so args must be pre-encoded with
   `serde_json::to_vec(&envelope)`. `max_attempts` defaults to 25 and `run_at` to now when
   unset. **Observed:** a push on `pool.begin()` (the workspace's own pool, since Task 1b)
   followed by rollback leaves no row. Because the workspace and apalis-postgres now share
   one sqlx, this transaction can be the same one an outbox delete runs in (see item 12).

6. **Scheduled push.** `TaskBuilder::run_at_time(SystemTime)`, `::run_after(Duration)`,
   `::run_in_seconds/minutes/hours(u64)`, or `::run_at_timestamp(unix_secs: u64)`. Precision
   is **whole seconds**. The row is `status = 'Pending'` with `run_at` in the future.
   **Observed:** `run_after(1h)` left the task `Pending`, attempts 0, `run_at > now() + 59
   min`, and it did not run. Pickup (**source**): the notify trigger fires only when `run_at
   <= now()` at insert, so a scheduled task is fetched on the worker's next wake. Wakes come
   from the heartbeat timer (`Config::heartbeat_interval`, default 30 s) or any
   notification or completion on that queue. Latency is therefore up to
   `heartbeat_interval`.

7. **Worker per queue with a concurrency limit.**
   ```rust
   use apalis::prelude::*; // WorkerBuilder, WorkerBuilderExt, Data, Attempt, TaskId, BoxDynError
   let worker = WorkerBuilder::new(unique_worker_name)      // &str / WorkerContext
       .backend(storage.with_pubsub())
       .data(shared_state)                                  // extract as Data<T>
       .concurrency(n)                                      // tower ConcurrencyLimitLayer (apalis "limit" feature, default)
       .build(handler);
   async fn handler(job: Envelope, attempt: Attempt, task_id: TaskId, state: Data<T>)
       -> Result<(), BoxDynError> { … }
   ```
   Extractors (`FromRequest`) include `Attempt`, `TaskId`, `WorkerContext`, and `Data<T>`.
   The attempt is `apalis::prelude::Attempt::current() -> usize`, which is **1 on the first
   run** and increments per run, whether an in-process retry or a DB re-fetch (**observed**).
   The cap is available inside a retry policy as `Task::max_attempts() -> Option<usize>`.
   To expose it to a handler, carry it in the envelope. (Unverified: the handler can
   `.data()`-extract `TaskContext`.)
   **Worker names must be unique across live processes.** `apalis.workers` upserts with
   `WHERE pg_try_advisory_lock(hashtext(id))`, and a name held elsewhere fails with
   `apalis_postgres::Error::WorkerAlreadyExists` (**source**). Include host/pid or a UUID.
   `Config::batch_size` rows are claimed per fetch (`status = 'Queued'`, `lock_by = worker`)
   before running. Set `batch_size` close to `concurrency` so one worker does not hoard
   queued rows.

8. **Retry.** (What Task 6 built is the last bullet.)
   - **Plain `Err(e)`**: the row becomes `status = 'Failed'`, `attempts = n`,
     `done_at = now()`. apalis-postgres **re-fetches `Failed AND attempts < max_attempts`
     once `run_at < now()`**. The ack does not move `run_at`, so for an untouched row the
     re-fetch is immediate. **Observed:** with `max_attempts(3)` and no retry layer, runs
     happened at attempts 1, 2, 3 back-to-back, ending `Failed`, attempts 3.
   - **Abort, do not retry**: `Err(Box::new(apalis::prelude::AbortError::new(e)))` makes the
     row `status = 'Killed'`, and it is never re-fetched. **Observed:** one run, `Killed`,
     attempts 1.
   - `RetryAfterError::new(e, dur)` / `DeferredError::new(e)` make the row `Pending`
     (**source**), but the ack never touches `run_at`, so **the duration is ignored**. Not
     used.
   - **Max attempts**: `TaskBuilder::max_attempts(n)`; default **25**; no "unlimited", so
     `i32::MAX` stands in.
   - **In-process backoff** (`.retry(policy)`, tower `Retry`, custom `Policy` reading the
     job): works (**observed** in the Task 1 probe), but the task stays `Running` holding a
     concurrency slot while it sleeps, and a crash loses the retry. **Not used.**
   - **What `postit-jobs` does (Task 6): DB-level deferral.** The relay pushes each task with
     `max_attempts` from the job type's `RetryPolicy` (`None` → 1, `Some(n)` → n,
     unlimited → `i32::MAX`). `backend::handle` maps the dispatcher's `Outcome`:
     - `Done` → `Ok(())`, row `Done`.
     - `RetryAfter(d)` → `apalis_sql::defer_next_run` runs `UPDATE apalis.jobs SET run_at =
       now() + d WHERE id = $1` on the task's own row, then returns a plain `Err`. The ack
       marks it `Failed`; `get_jobs`'s `run_at < now()` holds the re-fetch until `d` has
       passed, and the queue's poll interval then picks it up. No slot held; a crash cannot
       lose it. **Observed:** `backend::tests::deferring_run_at_delays_the_refetch` (raw
       apalis worker, 1.5 s deferral) measured a 1.55 s gap between attempts 1 and 2;
       `tests/worker.rs` `retries_follow_the_policy_and_report_the_last_attempt` asserts
       600 ms and 900 ms backoffs through the full `Worker`. If the `UPDATE` fails, the
       retry runs without its delay (logged); the attempt cap still applies.
     - `Abort(msg)` (`JobError::Fatal`, unknown job type, a handler panic caught inside
       `dispatch` around the handler call, or a
       `Retry` on the last allowed attempt) → `AbortError`, row `Killed`. So exhausted
       retries also end `Killed`, not `Failed` with `attempts >= max_attempts`.
     Attempt = `Attempt::current()` (1-based). The per-type cap lives in `dispatch`; the
     `max_attempts` column is a backstop. A re-enqueue after a crash/shutdown sets `Pending`
     with `attempts + 1`, and `get_jobs` fetches `Pending` rows whatever their attempts, so
     such a run can report an attempt above the cap; `dispatch` aborts it after that run.

9. **Graceful shutdown.** Single worker:
   `Worker::run_until(signal)`, where `signal: impl Future<Output = Result<(), E>> + Send +
   'static` and `E: Into<apalis::prelude::WorkerError>`. When the signal resolves it calls
   `WorkerContext::stop()`, and the worker finishes in-flight tasks and returns (**observed**:
   all three probe workers stop this way). Several workers:
   `apalis::prelude::Monitor::new().register(|_run| worker).shutdown_timeout(d).run_with_signal(signal)`,
   where `signal: Future<Output = std::io::Result<()>>` (**source**). Alternatively run each
   `run_until` future in a `JoinSet`. On close, tasks that were fetched but not started are
   put back to `Pending` with attempts + 1 (`reenqueue_abandoned`, **source**).

10. **Cleanup API for finished jobs: none.** The crate ships `queries/backend/vacuum.sql`,
    but no Rust function uses it, and it targets an unqualified `Jobs`. Raw-SQL fallback
    (in `apalis_sql.rs`):
    - Table: `apalis.jobs`. Status column: `status TEXT`.
    - Done: `'Done'`. Killed: `'Killed'`. Failed for good: `status = 'Failed' AND attempts >=
      max_attempts`. `'Failed'` with `attempts < max_attempts` is still retryable; do not
      purge it.
    - Completion timestamp: `done_at timestamptz`. It is set to `NOW()` on every ack,
      including a retryable failure, and cleared to `NULL` on re-enqueue.
    - Stale workers: `apalis.workers (id, worker_type, storage_name, layers, last_seen,
      started_at)`. `apalis.jobs.lock_by` has a foreign key to `apalis.workers(id)`, so
      delete jobs before deleting workers.

11. **Task states** (column `apalis.jobs.status`, TEXT, exact spellings from
    `apalis_core::task::status::Status` `Display`/`FromStr`):
    | Console state | Predicate |
    |---|---|
    | queued (ready) | `status = 'Pending' AND run_at <= now()` |
    | scheduled | `status = 'Pending' AND run_at > now()` |
    | claimed by a worker, not started | `status = 'Queued'` (`lock_by`, `lock_at` set) |
    | running | `status = 'Running'` |
    | done | `status = 'Done'` |
    | failed, will retry | `status = 'Failed' AND attempts < max_attempts` |
    | failed for good (dead) | `status = 'Failed' AND attempts >= max_attempts` |
    | killed | `status = 'Killed'` |

    Columns: `id TEXT` (ULID, PK), `job_type TEXT` (queue name), `job BYTEA` (JSON args),
    `status TEXT`, `attempts INT`, `max_attempts INT`, `run_at timestamptz`, `last_result
    JSONB` (the handler's `Result`, e.g. `{"Err": "…"}`), `lock_at timestamptz`, `lock_by
    TEXT`, `done_at timestamptz`, `priority INT`, `metadata hstore`, `idempotency_key TEXT`.
    For P8, `PostgresStorage` also implements `ListTasks`, `ListAllTasks`, `ListQueues`,
    `ListWorkers`, `Metrics`, `FetchById` and `WaitForCompletion` (**source**, not probed).

12. **Relay: "task-ID dedupe", one transaction per batch** (built in Task 6,
    `relay::drain_once`). The outbox and apalis share the workspace pool, so:
    1. `pool.begin()`; `JobOutboxRepo::claim_batch` takes up to 100 rows of registered job
       types, `FOR UPDATE SKIP LOCKED`.
    2. Per row, `Backend::push` opens a **savepoint** on that transaction and pushes one
       task via `apalis_postgres::queries::push_tasks(&mut *savepoint, queue, vec![task])`:
       `task_id = TaskId::from_ulid(Ulid::from(row.id))`, args
       `serde_json::to_vec(&Envelope)`, `max_attempts` from the `RetryPolicy`, and, when
       `run_at` is in the future, `run_at_timestamp` rounded **up** to whole seconds (a task
       never becomes due early). One task per call: a batch fails whole on one duplicate.
    3. `Ok` → release the savepoint (`Stored`). A unique violation whose constraint is
       exactly `unique_job_id` or `jobs_pkey` → roll back to the savepoint
       (`AlreadyStored`). Any other error (another or unnamed unique violation included:
       a false "already stored" would delete the row and lose the job) → roll back to the
       savepoint (`Failed`). Without the savepoint either error would abort the whole
       transaction. Only a failure of the savepoint itself (lost connection) is `Err`, and
       abandons the batch.
    4. `JobOutboxRepo::delete(row.id)` for `Stored`/`AlreadyStored`; a `Failed` row is
       logged and left in the outbox for the next pass, without holding up the rest of its
       batch (`relay::tests::a_rejected_row_stays_in_the_outbox_without_blocking_its_batch`).
       Commit per batch; loop until a claim comes back empty or a batch moved nothing (only
       rejected rows left).

    Crash before commit: push and delete roll back together; rows are re-pushed next pass.
    Crash after commit: nothing to redo. A task stored without its outbox delete
    (simulated by `relay::tests::a_row_pushed_before_a_crash_runs_once`) is `AlreadyStored`
    on the next pass and runs once. The handler recovers the `JobId` as
    `Uuid::from(task_id.as_ulid()?)`.
