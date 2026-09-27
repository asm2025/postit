//! Task 1 spike: pins down how apalis `=1.0.0-rc.10` + apalis-postgres `=1.0.0-rc.9`
//! behave against a real Postgres, so `APALIS_NOTES.md` states observed facts. Test-only.
//!
//! Task 1b moved the workspace to sqlx 0.9, the same sqlx apalis-postgres is built on, so
//! the per-test `sqlx::PgPool` is handed to apalis-postgres directly: no second pool.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use apalis::prelude::*;
use apalis_postgres::{Config, PostgresStorage};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ulid::Ulid;
use uuid::Uuid;

const QUEUE: &str = "postit::probe";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    job_type: String,
    payload: serde_json::Value,
}

fn envelope(job_type: &str) -> Envelope {
    Envelope {
        job_type: job_type.to_owned(),
        payload: serde_json::json!({ "n": 1 }),
    }
}

/// Runs apalis-postgres's migrations twice concurrently and once more, proving the call is
/// safe under concurrent callers and idempotent.
async fn migrate(pool: &PgPool) {
    let (a, b) = tokio::join!(PostgresStorage::setup(pool), PostgresStorage::setup(pool));
    a.unwrap_or_else(|e| unreachable!("setup a: {e}"));
    b.unwrap_or_else(|e| unreachable!("setup b: {e}"));
    PostgresStorage::setup(pool)
        .await
        .unwrap_or_else(|e| unreachable!("setup again: {e}"));
}

fn storage(pool: &PgPool) -> PostgresStorage<Envelope> {
    PostgresStorage::<Envelope>::new(pool).with_config(
        Config::default()
            .queue(QUEUE)
            .heartbeat_interval(Duration::from_secs(1)),
    )
}

/// Row state of one apalis task, read over the workspace's own pool.
async fn job_row(pool: &PgPool, id: &str) -> (String, i32, i32, bool) {
    sqlx::query_as::<_, (String, i32, i32, bool)>(
        "SELECT status, attempts, max_attempts, done_at IS NOT NULL FROM apalis.jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("job_row {id}: {e}"))
}

/// Polls `counter` until it reaches `target` or 10 s pass.
async fn wait_for(counter: &AtomicUsize, target: usize) {
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while counter.load(Ordering::SeqCst) < target {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
}

#[derive(Clone, Default)]
struct Seen {
    runs: Arc<AtomicUsize>,
    /// `(job_type, attempt, task id)` per handler call.
    calls: Arc<Mutex<Vec<(String, usize, String)>>>,
    /// When each handler call started.
    stamps: Arc<Mutex<Vec<std::time::Instant>>>,
}

#[derive(Debug)]
struct Boom;

impl std::fmt::Display for Boom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("boom")
    }
}

impl std::error::Error for Boom {}

async fn handle(
    job: Envelope,
    attempt: Attempt,
    task_id: TaskId,
    seen: Data<Seen>,
) -> Result<(), BoxDynError> {
    if let Ok(mut calls) = seen.calls.lock() {
        calls.push((job.job_type.clone(), attempt.current(), task_id.to_string()));
    }
    if let Ok(mut stamps) = seen.stamps.lock() {
        stamps.push(std::time::Instant::now());
    }
    seen.runs.fetch_add(1, Ordering::SeqCst);
    match job.job_type.as_str() {
        "fail" => Err(Box::new(Boom)),
        "abort" => Err(Box::new(AbortError::new(Boom))),
        _ => Ok(()),
    }
}

async fn run_worker_until(pool: &PgPool, seen: &Seen, target: usize) {
    let worker = WorkerBuilder::new("postit-probe-worker")
        .backend(storage(pool).with_pubsub())
        .data(seen.clone())
        .concurrency(2)
        .build(handle);
    let runs = seen.runs.clone();
    worker
        .run_until(async move {
            wait_for(&runs, target).await;
            Ok::<(), WorkerError>(())
        })
        .await
        .unwrap_or_else(|e| unreachable!("worker: {e}"));
}

/// An in-process retry policy whose delay comes from the job itself (`payload.delay_ms`),
/// retrying while the attempt is below the task's `max_attempts`.
#[derive(Clone)]
struct PayloadBackoff;

impl<Res, Err> apalis::layers::retry::Policy<Task<Envelope>, Res, Err> for PayloadBackoff {
    type Future = futures::future::BoxFuture<'static, ()>;

    fn retry(
        &mut self,
        req: &mut Task<Envelope>,
        result: &mut Result<Res, Err>,
    ) -> Option<Self::Future> {
        if result.is_ok() || req.attempt() >= req.max_attempts().unwrap_or(1) {
            return None;
        }
        let delay_ms = req.args.payload["delay_ms"].as_u64().unwrap_or(0);
        Some(Box::pin(tokio::time::sleep(Duration::from_millis(
            delay_ms,
        ))))
    }

    fn clone_request(&mut self, req: &Task<Envelope>) -> Option<Task<Envelope>> {
        Some(req.clone())
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn apalis_probe_retry_layer_with_payload_delay(pool: PgPool) {
    migrate(&pool).await;

    let id = Ulid::from(Uuid::now_v7());
    let job = Envelope {
        job_type: "fail".to_owned(),
        payload: serde_json::json!({ "delay_ms": 300 }),
    };
    storage(&pool)
        .push_task(
            TaskBuilder::new(job)
                .task_id(TaskId::from_ulid(id))
                .max_attempts(3)
                .build(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("push: {e}"));

    let seen = Seen::default();
    let runs = seen.runs.clone();
    let worker = WorkerBuilder::new("postit-probe-worker-3")
        .backend(storage(&pool).with_pubsub())
        .data(seen.clone())
        .retry(PayloadBackoff)
        .build(handle);
    worker
        .run_until(async move {
            wait_for(&runs, 3).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok::<(), WorkerError>(())
        })
        .await
        .unwrap_or_else(|e| unreachable!("worker: {e}"));

    let calls = seen
        .calls
        .lock()
        .unwrap_or_else(|e| unreachable!("calls lock: {e}"))
        .clone();
    println!("calls: {calls:?}");
    let attempts: Vec<usize> = calls.iter().map(|c| c.1).collect();
    assert_eq!(attempts, vec![1, 2, 3]);
    let stamps = seen
        .stamps
        .lock()
        .unwrap_or_else(|e| unreachable!("stamps lock: {e}"))
        .clone();
    let gaps: Vec<Duration> = stamps.windows(2).map(|w| w[1] - w[0]).collect();
    println!("gaps between attempts: {gaps:?}");
    assert!(gaps.iter().all(|g| *g >= Duration::from_millis(300)));

    // One ack after the in-process retries: the row records the final attempt count.
    let (status, attempts, _, done) = job_row(&pool, &id.to_string()).await;
    assert_eq!((status.as_str(), attempts, done), ("Failed", 3, true));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn apalis_probe_push_dedupe_and_run(pool: PgPool) {
    migrate(&pool).await;

    // Migrations live in apalis's own schema and history table.
    let history: i64 = sqlx::query_scalar("SELECT count(*) FROM apalis._sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("apalis history: {e}"));
    assert!(history > 0);

    // Caller-chosen task ID: the UUID v7's 128 bits reinterpreted as a ULID.
    let job_id = Uuid::now_v7();
    let ulid = Ulid::from(job_id);
    assert_eq!(Uuid::from(ulid), job_id, "UUID <-> ULID is lossless");

    let mut sink = storage(&pool);
    let first = TaskBuilder::new(envelope("ok"))
        .task_id(TaskId::from_ulid(ulid))
        .max_attempts(3)
        .build();
    sink.push_task(first)
        .await
        .unwrap_or_else(|e| unreachable!("first push: {e}"));

    // Same task ID again: the INSERT hits apalis.jobs's primary key.
    let second = TaskBuilder::new(envelope("ok"))
        .task_id(TaskId::from_ulid(ulid))
        .build();
    let Err(err) = sink.push_task(second).await else {
        unreachable!("duplicate task ID was accepted")
    };
    let TaskSinkError::PushError(apalis_postgres::Error::Database(sqlx_err)) = &err else {
        unreachable!("unexpected duplicate-push error shape: {err:?}")
    };
    let Some(db_err) = sqlx_err.as_database_error() else {
        unreachable!("not a database error: {sqlx_err:?}")
    };
    println!(
        "duplicate task id -> code={:?} constraint={:?} unique={}",
        db_err.code(),
        db_err.constraint(),
        db_err.is_unique_violation()
    );
    assert!(db_err.is_unique_violation());
    assert_eq!(db_err.code().as_deref(), Some("23505"));
    // apalis.jobs carries two unique indexes on `id` (`unique_job_id` from the first
    // migration, `jobs_pkey` added later); Postgres reports whichever it checks first
    // (observed: `unique_job_id`), so callers must accept either.
    assert!(matches!(
        db_err.constraint(),
        Some("unique_job_id" | "jobs_pkey")
    ));

    // The sink does not keep the failed task buffered: a later push on the same sink works.
    let third = TaskBuilder::new(envelope("ok"))
        .task_id(TaskId::from_ulid(Ulid::from(Uuid::now_v7())))
        .idempotency_key("probe-key")
        .build();
    sink.push_task(third)
        .await
        .unwrap_or_else(|e| unreachable!("push after duplicate: {e}"));

    // Duplicate idempotency key (per job_type) is the other unique index.
    let fourth = TaskBuilder::new(envelope("ok"))
        .task_id(TaskId::from_ulid(Ulid::from(Uuid::now_v7())))
        .idempotency_key("probe-key")
        .build();
    let Err(TaskSinkError::PushError(apalis_postgres::Error::Database(key_err))) =
        sink.push_task(fourth).await
    else {
        unreachable!("duplicate idempotency key was accepted")
    };
    let Some(key_db_err) = key_err.as_database_error() else {
        unreachable!("not a database error: {key_err:?}")
    };
    println!(
        "duplicate idempotency key -> code={:?} constraint={:?}",
        key_db_err.code(),
        key_db_err.constraint()
    );
    assert_eq!(key_db_err.constraint(), Some("idx_jobs_idempotency_key"));

    // Push inside a transaction: `apalis_postgres::queries::push_tasks` takes any sqlx
    // executor, with args pre-encoded to JSON bytes. Since Task 1b, that transaction can be
    // the caller's own (same pool, same sqlx version) rather than a second connection.
    // Rolled back -> nothing lands.
    let bytes = serde_json::to_vec(&envelope("ok")).unwrap_or_else(|e| unreachable!("json: {e}"));
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let in_tx = TaskBuilder::new(bytes)
        .task_id(TaskId::from_ulid(Ulid::from(Uuid::now_v7())))
        .build();
    apalis_postgres::queries::push_tasks(&mut *tx, QUEUE, vec![in_tx])
        .await
        .unwrap_or_else(|e| unreachable!("push in tx: {e}"));
    tx.rollback()
        .await
        .unwrap_or_else(|e| unreachable!("rollback: {e}"));

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM apalis.jobs WHERE job_type = $1")
        .bind(QUEUE)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(rows, 2, "only the first and third pushes landed");

    let seen = Seen::default();
    run_worker_until(&pool, &seen, 2).await;
    assert_eq!(seen.runs.load(Ordering::SeqCst), 2);

    let calls = seen
        .calls
        .lock()
        .unwrap_or_else(|e| unreachable!("calls lock: {e}"))
        .clone();
    println!("calls: {calls:?}");
    let Some((_, attempt, seen_id)) = calls.iter().find(|c| c.2 == ulid.to_string()) else {
        unreachable!("handler never saw the caller-chosen task id")
    };
    assert_eq!(*attempt, 1, "first run reports attempt 1");
    let parsed = Ulid::from_string(seen_id).unwrap_or_else(|e| unreachable!("ulid: {e}"));
    assert_eq!(Uuid::from(parsed), job_id);

    let (status, attempts, max_attempts, done) = job_row(&pool, &ulid.to_string()).await;
    assert_eq!(
        (status.as_str(), attempts, max_attempts, done),
        ("Done", 1, 3, true)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn apalis_probe_failure_abort_and_schedule(pool: PgPool) {
    migrate(&pool).await;

    let fail_id = Ulid::from(Uuid::now_v7());
    let abort_id = Ulid::from(Uuid::now_v7());
    let later_id = Ulid::from(Uuid::now_v7());
    let mut sink = storage(&pool);
    for task in [
        TaskBuilder::new(envelope("fail"))
            .task_id(TaskId::from_ulid(fail_id))
            .max_attempts(3)
            .build(),
        TaskBuilder::new(envelope("abort"))
            .task_id(TaskId::from_ulid(abort_id))
            .max_attempts(3)
            .build(),
        TaskBuilder::new(envelope("ok"))
            .task_id(TaskId::from_ulid(later_id))
            .run_after(Duration::from_secs(3600))
            .build(),
    ] {
        sink.push_task(task)
            .await
            .unwrap_or_else(|e| unreachable!("push: {e}"));
    }

    // 3 runs of "fail" (attempts 1..=3, no retry layer: apalis-postgres re-fetches
    // `Failed AND attempts < max_attempts`) + 1 run of "abort". Wait for 4, then linger
    // to catch any extra run.
    let seen = Seen::default();
    let runs = seen.runs.clone();
    let worker = WorkerBuilder::new("postit-probe-worker-2")
        .backend(storage(&pool).with_pubsub())
        .data(seen.clone())
        .build(handle);
    worker
        .run_until(async move {
            wait_for(&runs, 4).await;
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok::<(), WorkerError>(())
        })
        .await
        .unwrap_or_else(|e| unreachable!("worker: {e}"));

    let calls = seen
        .calls
        .lock()
        .unwrap_or_else(|e| unreachable!("calls lock: {e}"))
        .clone();
    println!("calls: {calls:?}");
    let fail_attempts: Vec<usize> = calls
        .iter()
        .filter(|c| c.0 == "fail")
        .map(|c| c.1)
        .collect();
    assert_eq!(fail_attempts, vec![1, 2, 3]);
    let abort_attempts: Vec<usize> = calls
        .iter()
        .filter(|c| c.0 == "abort")
        .map(|c| c.1)
        .collect();
    assert_eq!(abort_attempts, vec![1]);
    assert!(
        calls.iter().all(|c| c.0 != "ok"),
        "scheduled task ran early"
    );

    let (status, attempts, _, done) = job_row(&pool, &fail_id.to_string()).await;
    assert_eq!((status.as_str(), attempts, done), ("Failed", 3, true));
    let (status, attempts, _, done) = job_row(&pool, &abort_id.to_string()).await;
    assert_eq!((status.as_str(), attempts, done), ("Killed", 1, true));
    let (status, attempts, _, done) = job_row(&pool, &later_id.to_string()).await;
    assert_eq!((status.as_str(), attempts, done), ("Pending", 0, false));
    let scheduled_in_future: bool = sqlx::query_scalar(
        "SELECT run_at > now() + interval '59 minutes' FROM apalis.jobs WHERE id = $1",
    )
    .bind(later_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("run_at: {e}"));
    assert!(scheduled_in_future);
}
