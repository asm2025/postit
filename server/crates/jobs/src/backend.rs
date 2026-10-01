//! The job storage: apalis + apalis-postgres. This is the only module that names an apalis
//! type (raw SQL against apalis's tables lives in `apalis_sql`); nothing here is `pub`, so
//! no apalis type reaches `postit-jobs`'s public API. API facts: `APALIS_NOTES.md`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use apalis::prelude::{
    AbortError, Attempt, BoxDynError, Data, PollWith, Strategy, TaskBuilder, TaskId, WorkerBuilder,
    WorkerBuilderExt, WorkerError,
};
use apalis_postgres::{Config, PostgresStorage};
use chrono::{DateTime, Utc};
use futures::channel::mpsc;
use sqlx::postgres::PgListener;
use sqlx::{Connection, PgConnection, PgPool};
use tokio::sync::watch;
use tokio::task::JoinSet;
use ulid::Ulid;
use uuid::Uuid;

use crate::apalis_sql;
use crate::dispatch::{self, Envelope, Outcome};
use crate::error::JobsError;
use crate::health::{self, WorkerHealth};
use crate::job::{JobId, Queue};
use crate::registry::JobRegistry;

/// The channel apalis-postgres's insert trigger notifies on (`APALIS_NOTES.md` item 2).
const INSERT_CHANNEL: &str = "apalis::job::insert";

/// The two unique indexes on `apalis.jobs.id` (`APALIS_NOTES.md` item 4). Only a violation
/// of one of these means "a task with this ID is already stored"; any other error,
/// including another unique violation, is a failed push.
const TASK_ID_INDEXES: [&str; 2] = ["unique_job_id", "jobs_pkey"];

/// A failed worker (lost database, say) is rebuilt after at least this long.
const MIN_RESTART_DELAY: Duration = Duration::from_secs(1);

pub(crate) enum Pushed {
    Stored,
    AlreadyStored,
    /// The storage rejected this task. Its savepoint was rolled back, so the caller's
    /// transaction is still usable. Holds the rendered error.
    Failed(String),
}

/// Cheap handle: pushes go through the caller's connection, and `run` builds the per-queue
/// storages over the pool it is given.
#[derive(Clone)]
pub(crate) struct Backend {
    poll_interval: Duration,
}

impl Backend {
    /// Checks that the storage schema exists (`migrate` has run). `poll_interval` bounds how
    /// long a due task (scheduled, or a deferred retry) waits for a fetch when no insert
    /// notification wakes its queue.
    pub(crate) async fn connect(pool: &PgPool, poll_interval: Duration) -> Result<Self, JobsError> {
        if !apalis_sql::storage_ready(pool).await? {
            return Err(JobsError::Backend(
                "job storage is not migrated: call postit_jobs::migrate first".into(),
            ));
        }
        Ok(Self { poll_interval })
    }

    /// Stores `envelope` under task ID `id`, due at `run_at` (rounded up to whole seconds,
    /// the storage's precision). Runs in a savepoint on `conn`, so a rejected push leaves the
    /// caller's transaction usable: a task already stored under `id` is `AlreadyStored`, any
    /// other storage error is `Failed`. `max_attempts: None` means unlimited.
    ///
    /// `Err` only when `conn` itself fails (the savepoint cannot be opened, released or
    /// rolled back), i.e. the caller's transaction is lost.
    pub(crate) async fn push(
        &self,
        conn: &mut PgConnection,
        queue: Queue,
        id: Uuid,
        envelope: &Envelope,
        run_at: DateTime<Utc>,
        max_attempts: Option<u32>,
    ) -> Result<Pushed, JobsError> {
        let mut task = TaskBuilder::new(serde_json::to_vec(envelope)?)
            .task_id(TaskId::from_ulid(Ulid::from(id)))
            .max_attempts(storage_max_attempts(max_attempts));
        if run_at > Utc::now() {
            task = task.run_at_timestamp(ceil_unix_seconds(run_at));
        }

        let mut savepoint = conn.begin().await?;
        match apalis_postgres::queries::push_tasks(
            &mut *savepoint,
            &queue_name(queue),
            vec![task.build()],
        )
        .await
        {
            Ok(()) => {
                savepoint.commit().await?;
                Ok(Pushed::Stored)
            }
            Err(err) if is_duplicate_task_id(&err) => {
                savepoint.rollback().await?;
                Ok(Pushed::AlreadyStored)
            }
            Err(err) => {
                savepoint.rollback().await?;
                Ok(Pushed::Failed(err.to_string()))
            }
        }
    }

    /// Runs one worker per queue until `shutdown` becomes `true`, then lets in-flight
    /// handlers finish. A worker that fails (database lost) is rebuilt under a new name.
    pub(crate) async fn run(
        self,
        pool: PgPool,
        registry: Arc<JobRegistry>,
        concurrency: HashMap<Queue, u32>,
        shutdown: watch::Receiver<bool>,
        health: WorkerHealth,
    ) -> Result<(), JobsError> {
        let wakers = Wakers::default();
        let state = HandlerState {
            pool: pool.clone(),
            registry,
        };
        let mut tasks = JoinSet::new();
        tasks.spawn(forward_inserts(
            pool,
            wakers.clone(),
            self.poll_interval,
            shutdown.clone(),
        ));
        for queue in Queue::ALL {
            tasks.spawn(run_queue(QueueWorker {
                queue,
                concurrency: concurrency.get(&queue).copied().unwrap_or(1).max(1),
                poll_interval: self.poll_interval,
                state: state.clone(),
                wakers: wakers.clone(),
                shutdown: shutdown.clone(),
            }));
        }
        health::supervise(tasks, shutdown, health, "queue worker").await;
        Ok(())
    }
}

/// Runs the storage's own migrations (schema `apalis`, history in `apalis._sqlx_migrations`).
pub(crate) async fn migrate_storage(pool: &PgPool) -> Result<(), JobsError> {
    PostgresStorage::setup(pool)
        .await
        .map_err(|err| JobsError::Backend(err.to_string()))
}

/// Deletes finished jobs from the storage: succeeded before `succeeded_before`, or
/// killed/exhausted-retries before `failed_before`. A `Failed` row still eligible for retry
/// is never touched. Returns the number of rows deleted.
pub(crate) async fn purge_finished(
    pool: &PgPool,
    succeeded_before: DateTime<Utc>,
    failed_before: DateTime<Utc>,
) -> Result<u64, JobsError> {
    Ok(apalis_sql::purge_finished(pool, succeeded_before, failed_before).await?)
}

fn queue_name(queue: Queue) -> String {
    format!("postit::{}", queue.as_str())
}

/// The storage has no "unlimited"; `i32::MAX` (its column type's maximum) stands in.
fn storage_max_attempts(max_attempts: Option<u32>) -> usize {
    let cap = max_attempts.map_or(i32::MAX, |n| i32::try_from(n).unwrap_or(i32::MAX));
    usize::try_from(cap).unwrap_or(usize::MAX)
}

/// Rounded up, so a task never becomes due before `run_at`.
fn ceil_unix_seconds(run_at: DateTime<Utc>) -> u64 {
    let seconds = run_at.timestamp() + i64::from(run_at.timestamp_subsec_nanos() > 0);
    u64::try_from(seconds).unwrap_or(0)
}

fn is_duplicate_task_id(err: &apalis_postgres::Error) -> bool {
    let apalis_postgres::Error::Database(err) = err else {
        return false;
    };
    err.as_database_error().is_some_and(|db| {
        db.is_unique_violation()
            && db
                .constraint()
                .is_some_and(|name| TASK_ID_INDEXES.contains(&name))
    })
}

/// Per-queue wake-up senders, fed by one shared listener (`forward_inserts`) so the whole
/// process holds one LISTEN connection for the storage instead of one per queue.
#[derive(Clone, Default)]
struct Wakers(Arc<Mutex<HashMap<String, mpsc::Sender<()>>>>);

impl Wakers {
    /// A fresh wake-up stream for `queue`, replacing any previous one.
    fn subscribe(&self, queue: Queue) -> mpsc::Receiver<()> {
        // Capacity 1: wake-ups coalesce. A woken worker keeps fetching batches until one
        // comes back empty, so one pending wake-up covers any number of inserts.
        let (tx, rx) = mpsc::channel(1);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(queue_name(queue), tx);
        rx
    }

    fn wake(&self, queue_name: &str) {
        let mut senders = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = senders.get_mut(queue_name) {
            // Full means a wake-up is already pending; disconnected means the worker is
            // being rebuilt and will fetch on start. Either way nothing is lost.
            let _ = tx.try_send(());
        }
    }
}

#[derive(serde::Deserialize)]
struct InsertEvent {
    job_type: String,
}

/// Wakes a queue's worker on every insert notification for that queue, until `shutdown`.
/// Without it a queue still fetches every poll interval; this makes new tasks start at once.
async fn forward_inserts(
    pool: PgPool,
    wakers: Wakers,
    retry_every: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let listener = async {
            let mut listener = PgListener::connect_with(&pool).await?;
            listener.listen(INSERT_CHANNEL).await?;
            Ok::<_, sqlx::Error>(listener)
        };
        let mut listener = tokio::select! {
            () = stopped(&mut shutdown) => return,
            connected = listener => match connected {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::warn!(error = %err, "job storage listener could not connect");
                    tokio::select! {
                        () = stopped(&mut shutdown) => return,
                        () = tokio::time::sleep(retry_every) => continue,
                    }
                }
            },
        };
        // All queues fetch on reconnect: notifications sent while disconnected are lost.
        for queue in Queue::ALL {
            wakers.wake(&queue_name(queue));
        }
        loop {
            tokio::select! {
                () = stopped(&mut shutdown) => return,
                received = listener.recv() => match received {
                    Ok(notification) => {
                        if let Ok(event) = serde_json::from_str::<InsertEvent>(notification.payload()) {
                            wakers.wake(&event.job_type);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "job storage listener lost its connection");
                        break;
                    }
                },
            }
        }
    }
}

/// Resolves once `shutdown` is `true` (or its sender is gone).
async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

#[derive(Clone)]
struct HandlerState {
    pool: PgPool,
    registry: Arc<JobRegistry>,
}

struct QueueWorker {
    queue: Queue,
    concurrency: u32,
    poll_interval: Duration,
    state: HandlerState,
    wakers: Wakers,
    shutdown: watch::Receiver<bool>,
}

async fn run_queue(mut this: QueueWorker) {
    let concurrency = usize::try_from(this.concurrency).unwrap_or(1);
    loop {
        if *this.shutdown.borrow() {
            return;
        }
        // Claim no more than can run at once, so one process does not hoard due tasks.
        let config = Config::default()
            .queue(queue_name(this.queue))
            .batch_size(concurrency);
        let storage = PostgresStorage::<Envelope>::new(&this.state.pool).with_config(config);
        // Interval first: `Strategy` stops at the first ready source, so a source listed after
        // a closed stream would never re-arm its timer.
        let strategy = Strategy::new()
            .interval(this.poll_interval)
            .stream(this.wakers.subscribe(this.queue));
        // Worker names must be unique across live processes (`APALIS_NOTES.md` item 7).
        let name = format!("postit-{}-{}", this.queue.as_str(), Uuid::now_v7());
        let worker = WorkerBuilder::new(name.as_str())
            .backend(PollWith::new(storage, strategy))
            .data(this.state.clone())
            .concurrency(concurrency)
            .build(handle);
        let mut signal = this.shutdown.clone();
        let result = worker
            .run_until(async move {
                stopped(&mut signal).await;
                Ok::<(), WorkerError>(())
            })
            .await;
        match result {
            Ok(()) => return,
            Err(err) => {
                tracing::error!(queue = this.queue.as_str(), worker = %name, error = %err,
                    "job worker failed; rebuilding it");
                tokio::select! {
                    () = stopped(&mut this.shutdown) => return,
                    () = tokio::time::sleep(this.poll_interval.max(MIN_RESTART_DELAY)) => {}
                }
            }
        }
    }
}

/// A failure message stored as the task's result. Holds no user content (see `JobError`).
#[derive(Debug)]
struct Failure(String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

/// The storage-facing handler: maps the dispatcher's `Outcome` onto apalis results.
/// - `Done`: success.
/// - `RetryAfter(d)`: moves this task's `run_at` to `now() + d`, then a plain error. The
///   storage marks it `Failed` and re-fetches it once `run_at` passes (`APALIS_NOTES.md`
///   item 8). No in-process sleep: the slot is freed and a crash cannot lose the retry.
/// - `Abort`: `AbortError`, so the task ends `Killed` and is never re-fetched.
async fn handle(
    envelope: Envelope,
    attempt: Attempt,
    task_id: TaskId,
    state: Data<HandlerState>,
) -> Result<(), BoxDynError> {
    let Some(ulid) = task_id.as_ulid() else {
        return Err(Box::new(AbortError::new(Failure(format!(
            "task id `{task_id}` is not a ULID"
        )))));
    };
    let attempt = u32::try_from(attempt.current()).unwrap_or(u32::MAX);
    let job_id = JobId(Uuid::from(ulid));
    // `dispatch` turns a panicking handler into `Abort`, so the task ends `Killed` and the
    // worker keeps running.
    match dispatch::dispatch(&state.pool, &state.registry, envelope, job_id, attempt).await {
        Outcome::Done => Ok(()),
        Outcome::RetryAfter(delay) => {
            if let Err(err) =
                apalis_sql::defer_next_run(&state.pool, &ulid.to_string(), delay).await
            {
                tracing::warn!(error = %err, job_id = %job_id.0,
                    "could not defer a retry; it will run without its backoff delay");
            }
            Err(Box::new(Failure(format!(
                "attempt {attempt} failed; retrying in {delay:?}"
            ))))
        }
        Outcome::Abort(message) => Err(Box::new(AbortError::new(Failure(message)))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use apalis::prelude::{
        Attempt, BoxDynError, Data, PollWith, Strategy, TaskBuilder, TaskId, TaskSink,
        WorkerBuilder, WorkerError,
    };
    use apalis_postgres::{Config, PostgresStorage};
    use serde::{Deserialize, Serialize};
    use sqlx::PgPool;
    use ulid::Ulid;
    use uuid::Uuid;

    use postit_core::SystemIdGenerator;

    use crate::apalis_sql;
    use crate::testkit::{RunningWorker, wait_until};
    use crate::{Job, JobQueue, JobRegistry, Queue, RetryPolicy};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Probe {
        n: u32,
    }

    #[derive(Clone)]
    struct State {
        pool: PgPool,
        runs: Arc<Mutex<Vec<(usize, Instant)>>>,
    }

    #[derive(Debug)]
    struct Again;
    impl std::fmt::Display for Again {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("again")
        }
    }
    impl std::error::Error for Again {}

    async fn deferring(
        _job: Probe,
        attempt: Attempt,
        task_id: TaskId,
        state: Data<State>,
    ) -> Result<(), BoxDynError> {
        if let Ok(mut runs) = state.runs.lock() {
            runs.push((attempt.current(), Instant::now()));
        }
        apalis_sql::defer_next_run(
            &state.pool,
            &task_id.to_string(),
            Duration::from_millis(1500),
        )
        .await?;
        Err(Box::new(Again))
    }

    #[derive(Serialize, Deserialize)]
    struct Explodes {}
    impl Job for Explodes {
        const JOB_TYPE: &'static str = "explodes";
        const QUEUE: Queue = Queue::Default;
    }

    #[derive(Serialize, Deserialize)]
    struct After {}
    impl Job for After {
        const JOB_TYPE: &'static str = "after";
        const QUEUE: Queue = Queue::Default;
    }

    /// A panicking handler ends its task `Killed` (not retried, although its policy allows
    /// retries) and the worker goes on to run the next job.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_panicking_job_is_killed_and_the_worker_keeps_going(pool: PgPool) {
        let explosions = Arc::new(AtomicUsize::new(0));
        let afters = Arc::new(AtomicUsize::new(0));
        let mut registry = JobRegistry::default();
        let counter = Arc::clone(&explosions);
        registry
            .register(
                RetryPolicy::Backoff {
                    max_attempts: Some(3),
                    initial: Duration::from_millis(100),
                    max: Duration::from_millis(100),
                },
                move |_: Explodes, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    async { unreachable!("handler panics on purpose") }
                },
            )
            .unwrap_or_else(|e| unreachable!("register explodes: {e}"));
        let counter = Arc::clone(&afters);
        registry
            .register(RetryPolicy::None, move |_: After, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            })
            .unwrap_or_else(|e| unreachable!("register after: {e}"));
        let worker = RunningWorker::start(pool.clone(), registry).await;
        let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));

        let exploded = queue
            .enqueue(&Explodes {})
            .await
            .unwrap_or_else(|e| unreachable!("enqueue explodes: {e}"));
        assert!(
            wait_until(Duration::from_secs(10), || explosions
                .load(Ordering::SeqCst)
                == 1)
            .await
        );
        queue
            .enqueue(&After {})
            .await
            .unwrap_or_else(|e| unreachable!("enqueue after: {e}"));
        assert!(
            wait_until(Duration::from_secs(10), || afters.load(Ordering::SeqCst)
                == 1)
            .await
        );

        let task_id = Ulid::from(exploded.0).to_string();
        let killed = wait_until_async(Duration::from_secs(5), || async {
            apalis_sql::task_status(&pool, &task_id)
                .await
                .ok()
                .flatten()
                .as_deref()
                == Some("Killed")
        })
        .await;
        assert!(killed);
        assert_eq!(explosions.load(Ordering::SeqCst), 1);
        worker.stop().await;
    }

    /// The ack is written asynchronously after the handler returns, so poll for it.
    async fn wait_until_async<F, Fut>(timeout: Duration, condition: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        condition().await
    }

    /// Verifies the retry-delay mechanism the dispatcher relies on: a handler that moves its
    /// own row's `run_at` forward before returning a plain `Err` is not re-fetched until then.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn deferring_run_at_delays_the_refetch(pool: PgPool) {
        PostgresStorage::setup(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("setup: {e}"));
        let queue = "postit::defer-probe";
        let storage =
            PostgresStorage::<Probe>::new(&pool).with_config(Config::default().queue(queue));
        let mut sink = storage.clone();
        let id = Ulid::from(Uuid::now_v7());
        sink.push_task(
            TaskBuilder::new(Probe { n: 1 })
                .task_id(TaskId::from_ulid(id))
                .max_attempts(2)
                .build(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("push: {e}"));

        let state = State {
            pool: pool.clone(),
            runs: Arc::new(Mutex::new(Vec::new())),
        };
        let runs = Arc::clone(&state.runs);
        let backend = PollWith::new(
            storage,
            Strategy::new().interval(Duration::from_millis(100)),
        );
        let worker = WorkerBuilder::new(format!("defer-probe-{}", Uuid::now_v7()))
            .backend(backend)
            .data(state)
            .build(deferring);
        let watch = Arc::clone(&runs);
        worker
            .run_until(async move {
                let deadline = Instant::now() + Duration::from_secs(10);
                while Instant::now() < deadline && watch.lock().map_or(0, |r| r.len()) < 2 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                Ok::<(), WorkerError>(())
            })
            .await
            .unwrap_or_else(|e| unreachable!("worker: {e}"));

        let runs = runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let attempts: Vec<usize> = runs.iter().map(|r| r.0).collect();
        assert_eq!(attempts, vec![1, 2]);
        let gap = runs[1].1 - runs[0].1;
        println!("gap between attempts: {gap:?}");
        assert!(
            gap >= Duration::from_millis(1500),
            "re-fetched after {gap:?}"
        );
        assert!(
            gap < Duration::from_millis(2500),
            "re-fetch too late: {gap:?}"
        );
    }
}
