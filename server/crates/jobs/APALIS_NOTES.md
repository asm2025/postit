# apalis notes (pinned: apalis =1.0.0-rc.10, apalis-postgres =1.0.0-rc.9)

Resolved alongside: `apalis-core 1.0.0-rc.10`, `apalis-codec 0.1.0-rc.10`, `ulid 3.0.0`,
**`sqlx 0.9.0`**, which is also the workspace's own sqlx (Task 1b: the workspace moved from
sqlx 0.8 to 0.9 so there is exactly one sqlx in the dependency graph;
`cargo tree -i sqlx` shows a single `sqlx v0.9.0` used by `postit-data`, `postit-identity`,
`postit-jobs`, and `apalis-postgres`. **Observed.**).

Evidence: the probe tests in `src/apalis_probe.rs` (run with
`cargo test -p postit-jobs apalis_probe -- --nocapture`), marked **observed** below, and
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
   result, so it panics on a connection failure. Prefer `with_pubsub()`.

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

8. **Retry.**
   - **Plain `Err(e)`**: the row becomes `status = 'Failed'`, `attempts = n`,
     `done_at = now()`. apalis-postgres **re-fetches `Failed AND attempts < max_attempts`
     immediately**: `run_at` is not moved and there is no backoff. **Observed:** with
     `max_attempts(3)` and no retry layer, runs happened at attempts 1, 2, 3 back-to-back,
     ending `Failed`, attempts 3.
   - **Abort, do not retry**: `Err(Box::new(apalis::prelude::AbortError::new(e)))` makes the
     row `status = 'Killed'`, and it is never re-fetched. **Observed:** one run, `Killed`,
     attempts 1. The built-in retry policies also stop on `AbortError`.
   - `apalis::prelude::RetryAfterError::new(e, dur)` / `DeferredError::new(e)` make the row
     `status = 'Pending'` (**source**, `worker/lifecycle.rs`). apalis-postgres's ack
     (`handle_result.sql`) never touches `run_at`, so **the duration is ignored** and it is
     re-fetched at once. Do not rely on it for backoff.
   - **Max attempts**: `TaskBuilder::max_attempts(n)` sets the `max_attempts` column. The
     default when unset is **25**. There is no "unlimited", so use a large value
     (`i32::MAX`) for `delete_user`.
   - **Per-attempt backoff, in process**: `.retry(policy)` on the builder
     (`WorkerBuilderExt::retry`) wraps the handler in tower's `Retry`. Built-ins in
     `apalis::layers::retry`: `RetryPolicy::retries(n)`, `.with_backoff(B:
     tower::retry::backoff::Backoff)`, `.retry_if(pred)`, and `.from_task_config()` (reads
     `RetryMetadataExt::retries(n)` set on the `TaskBuilder`). Their `Backoff` cannot see the
     job. **The delay can depend on job data** with a custom
     `impl<Res, Err> apalis::layers::retry::Policy<Task<Envelope>, Res, Err>` (the tower
     trait is re-exported). `retry(&mut self, req: &mut Task<Envelope>, result)` sees
     `req.args`, `req.attempt()` and `req.max_attempts()`, and returns
     `Some(Box::pin(sleep(delay)))` to retry. **Observed** (`PayloadBackoff` in the probe):
     delay taken from `payload.delay_ms` (300 ms) gave gaps of about 307 ms and 312 ms
     between attempts 1→2→3. There is one ack at the end: the row ends `Failed`, attempts 3.
     Caveats (**source**): during the delay the task stays `Running` and holds a concurrency
     slot. A crash loses the in-process retry: after `heartbeat_interval × missed_heartbeats`
     the orphan is re-enqueued as `Pending`, attempts + 1. The policy must stop at
     `attempt >= max_attempts`, otherwise the DB-level re-fetch (first bullet) adds more runs.
     Built-in policies also stop when the worker is shutting down.
   - **Long backoff (minutes to hours)**: an in-process sleep is unsuitable. A DB-level
     option, **not probed**: before returning `Err`, the handler updates its own row's
     `run_at = now() + delay` (raw SQL in `apalis_sql.rs`). The ack does not reset `run_at`,
     and `get_jobs` requires `run_at < now()`, so the re-fetch waits. Task 6 must verify this
     before relying on it.

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

12. **Chosen relay path: "task-ID dedupe"** (spec Risk mitigations 1, default design). Since
    Task 1b puts the outbox and apalis on the same sqlx pool, all four steps run in one
    transaction on that pool. Per outbox row:
    1. In a transaction on the workspace pool, `SELECT … FOR UPDATE SKIP LOCKED`.
    2. Push a single task with `task_id = TaskId::from_ulid(Ulid::from(row.id))` and
       `max_attempts`/`run_at` from the row, via `apalis_postgres::queries::push_tasks(&mut
       *tx, queue, vec![task])` (item 5) on that same transaction, with args pre-encoded to
       JSON bytes.
    3. Treat `Ok` or a unique violation on `unique_job_id`/`jobs_pkey` as "stored".
    4. Delete the outbox row and commit.

    A relay that crashed after the push but before the commit rolls the whole transaction
    back, so nothing lands and the row is re-pushed on the next relay pass — no duplicate
    ever reaches apalis. A relay that crashed after commit has nothing left to redo: push
    and delete committed together. The handler recovers the `JobId` as
    `Uuid::from(task_id.as_ulid()?)`.
