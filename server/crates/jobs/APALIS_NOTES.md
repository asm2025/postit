# apalis notes (pinned: apalis =1.0.0-rc.10, apalis-postgres =1.0.0-rc.9)

Resolved alongside: `apalis-core 1.0.0-rc.10`, `apalis-codec 0.1.0-rc.10`, `ulid 3.0.0`,
**`sqlx 0.9.0`** (apalis-postgres's own sqlx).

Evidence: the probe tests in `src/apalis_probe.rs` (run with
`cargo test -p postit-jobs apalis_probe -- --nocapture`), marked **observed** below, and
the crate sources under `~/.cargo/registry/src/index.crates.io-*/apalis-{core,postgres}-*`,
marked **source**. Paths below are public paths unless noted.

## Read first: two constraints the pins impose

**A. apalis-postgres rc.9 is built on sqlx 0.9; the workspace is on sqlx 0.8.** The two
are separate crates with separate types. Consequences:

- apalis-postgres needs its own pool, an `apalis_postgres::PgPool` (a re-export of sqlx 0.9
  `PgPool`). `postit_data::Db::pool()` (sqlx 0.8) cannot be passed in. Build the pool from
  the re-exported `apalis_postgres::PgConnectOptions` (`::new().host().port().database()
  .username().password()`, the same builder shape as `postit_data::pool::Db::connect`) and
  `apalis_postgres::PgPool::connect_with(options)`. No direct `sqlx 0.9` dependency is needed.
  Because that means building connection options outside `postit-data` (its doc comment says
  that is the only place), this needs a ruling.
- A `postit_jobs::migrate(pool: &sqlx::PgPool)` taking the workspace pool cannot run
  apalis's migrations with that pool alone. The pool does not expose its password, so it
  cannot be turned into a 0.9 pool either. `migrate` must take something that can build a
  0.9 pool, such as `DatabaseSettings` or a prebuilt apalis pool held by a `postit-jobs` type.
  Global Constraints' "`postit_jobs::migrate(&pool)` in tests" needs adjusting accordingly.
- **Nothing apalis-side can join a sqlx 0.8 transaction.** Outbox work (0.8) and apalis
  pushes (0.9) are always two transactions on two connections. The relay design (task-ID
  dedupe, item 12) already assumes this.
- In `#[sqlx::test]` tests, the probe builds the 0.9 pool from `DATABASE_URL` plus the
  per-test database name, `pool.connect_options().get_database()`. See `apalis_pool` in
  `src/apalis_probe.rs`.
- Raw SQL against `apalis.*` tables can use the workspace's 0.8 pool. The probe reads
  `apalis.jobs` that way.
- Alternative (not adopted, needs a ruling): `apalis =1.0.0-rc.9` + `apalis-core =1.0.0-rc.9`
  + `apalis-sql =1.0.0-rc.9` + `apalis-postgres =1.0.0-rc.8` is on sqlx 0.8 and passes
  `cargo check` in a scratch crate. `apalis-postgres rc.8` with `apalis rc.10` does **not**
  compile, because `apalis-sql rc.9` breaks against `apalis-core rc.10`. rc.8 has no
  `sqlx.toml`, so its migration history would share `public._sqlx_migrations` with
  `postit-data`. Both migrators would then need `set_ignore_missing(true)`. This path was
  not probed.

**B. apalis-postgres task IDs must be ULIDs.** `BackendConfig::Id = ulid::Ulid`.
`PgTaskRow::try_into` (`src/from_row.rs`) decodes `apalis.jobs.id` with
`Ulid::from_string` and fails with `Error::TaskIdError` otherwise. A hyphenated UUID string
would be stored fine but break every later fetch. A UUID v7 maps losslessly:
`ulid::Ulid::from(uuid)` / `uuid::Uuid::from(ulid)`, via ulid's `uuid` feature, which
apalis-core enables. The ULID keeps the v7 millisecond prefix. **Observed:** round-trip
equality, and the handler's `TaskId` parses back to the original UUID. This is why
`postit-jobs` depends on `ulid = "3"` (workspace dep, `features = ["uuid"]`).

## Answers

1. **Migrations.** `apalis_postgres::PostgresStorage::setup(&apalis_postgres::PgPool)
   -> Result<(), apalis_postgres::Error>` (impl on `PostgresStorage<()>`, feature
   `migrate`, on by default). `PostgresStorage::migrations() -> sqlx::migrate::Migrator` (0.9)
   gives the migrator without running it. Schema: **`apalis`**, holding tables `apalis.jobs`
   and `apalis.workers` and functions `apalis.get_jobs`, `apalis.push_job`,
   `apalis.generate_ulid`, and `apalis.notify_new_jobs`, plus trigger `notify_workers`.
   History table: **`apalis._sqlx_migrations`**, set by the crate's `sqlx.toml`, so it is
   separate from `postit-data`'s `public._sqlx_migrations`. **Observed:** it exists after
   `setup`. Idempotent and concurrency-safe: the sqlx 0.9 `Migrator::run_direct` takes
   `pg_advisory_lock(generate_lock_id(current_database()))` before creating the schema or
   history table. That is the same key sqlx 0.8's migrator uses, so it also serialises with
   `postit-data`'s migrations. **Observed:** two concurrent `setup` calls (`tokio::join!`)
   plus a third call all succeed. Note: a migration runs `CREATE EXTENSION IF NOT EXISTS
   hstore`. hstore is a trusted extension (PG13+), so the database owner can create it, but
   the role running `setup` needs `CREATE` on the database.

2. **Storage with LISTEN/NOTIFY fetch.**
   `PostgresStorage::<Envelope>::new(&apalis_pool).with_config(apalis_postgres::Config::default().queue("postit::mail")).with_pubsub()`
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

5. **Push inside a caller's transaction: only a sqlx 0.9 one.**
   `apalis_postgres::queries::push_tasks(conn: &mut E, queue: &str, tasks: Vec<apalis_postgres::PgTask>)`
   where `for<'e> &'e mut E: sqlx::Executor<'e, Database = sqlx::Postgres> + Send` (sqlx
   0.9). `PgTask = Task<Vec<u8>>`, so args must be pre-encoded with
   `serde_json::to_vec(&envelope)`. `max_attempts` defaults to 25 and `run_at` to now when
   unset. **Observed:** a push on `apalis_pool.begin()` followed by rollback leaves no row.
   A sqlx 0.8 transaction, which is ours, **cannot** be used (constraint A).

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

12. **Chosen relay path: "task-ID dedupe"** (spec Risk mitigations 1, default design). Per
    outbox row:
    1. In a sqlx 0.8 transaction, `SELECT … FOR UPDATE SKIP LOCKED`.
    2. Push a single task with `task_id = TaskId::from_ulid(Ulid::from(row.id))` and
       `max_attempts`/`run_at` from the row. The push goes through the queue's storage on
       the apalis (0.9) pool.
    3. Treat `Ok` or a unique violation on `unique_job_id`/`jobs_pkey` as "stored".
    4. Delete the outbox row and commit.

    A relay that crashed after the push but before the commit re-pushes, gets the
    conflict, and deletes the row, so the job runs once. The handler recovers the `JobId`
    as `Uuid::from(task_id.as_ulid()?)`.
