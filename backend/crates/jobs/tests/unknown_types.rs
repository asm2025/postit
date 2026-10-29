//! A job of a type the running worker does not register is re-queued, not killed, while a
//! newer worker may still pick it up (a rolling deploy); it is killed once it is too old.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use postit_core::SystemIdGenerator;
use postit_jobs::testkit::{
    RunningWorker, jobs_settings, relay_once, run_job_history_purge, wait_until_async,
};
use postit_jobs::{Job, JobContext, JobError, JobId, JobQueue, JobRegistry, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ulid::Ulid;

/// A type a newer release adds: the older worker does not register it.
#[derive(Serialize, Deserialize)]
struct FutureJob {}
impl Job for FutureJob {
    const JOB_TYPE: &'static str = "future_job";
    const QUEUE: Queue = Queue::Default;
}

/// Worker tests hold many connections each; run them one at a time.
static WORKERS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    )
}

/// A registry that knows `future_job`; every run is recorded as its attempt number.
fn newer_registry(retry: RetryPolicy, fail: bool, seen: Arc<Mutex<Vec<u32>>>) -> JobRegistry {
    let mut registry = JobRegistry::default();
    registry
        .register(retry, move |_: FutureJob, ctx: JobContext| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(ctx.attempt);
                if fail {
                    Err(JobError::Retry("try again".into()))
                } else {
                    Ok(())
                }
            }
        })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    registry
}

async fn state_of(pool: &PgPool, id: JobId) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(id.0)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

async fn attempt_rows(pool: &PgPool, id: JobId) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = $1")
        .bind(id.0)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

/// `(status, attempts, max_attempts)` of the storage row.
async fn storage_row(pool: &PgPool, id: JobId) -> Option<(String, i32, i32)> {
    sqlx::query_as("SELECT status, attempts, max_attempts FROM apalis.jobs WHERE id = $1")
        .bind(Ulid::from(id.0).to_string())
        .fetch_optional(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

/// Makes the storage row due now, as if its 60 seconds had passed.
async fn make_due(pool: &PgPool, id: JobId) {
    sqlx::query("UPDATE apalis.jobs SET run_at = now() - interval '1 second' WHERE id = $1")
        .bind(Ulid::from(id.0).to_string())
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
}

/// Enqueues a `future_job` and relays it with `newer`'s registry, as a newer worker would.
async fn relayed_by_newer_worker(pool: &PgPool, newer: &JobRegistry) -> JobId {
    let id = queue(pool)
        .enqueue(&FutureJob {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));
    assert_eq!(relay_once(pool, newer).await, 1);
    id
}

/// Runs an older worker (no `future_job`) until it has re-queued the job `times` times.
async fn older_worker_requeues(pool: &PgPool, id: JobId, times: i32) {
    let worker = RunningWorker::start(pool.clone(), JobRegistry::default()).await;
    for round in 1..=times {
        let requeued = wait_until_async(Duration::from_secs(15), || async {
            // The ack follows the handler: `attempts` is the storage's count of runs so far.
            matches!(storage_row(pool, id).await, Some((status, attempts, _))
                if status == "Failed" && attempts == round)
        })
        .await;
        assert!(requeued, "round {round}: {:?}", storage_row(pool, id).await);
        if round < times {
            make_due(pool, id).await;
        }
    }
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_type_is_requeued_without_consuming_an_attempt(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let newer = newer_registry(RetryPolicy::None, false, Arc::default());
    let id = relayed_by_newer_worker(&pool, &newer).await;

    older_worker_requeues(&pool, id, 1).await;

    assert_eq!(state_of(&pool, id).await, "queued");
    assert_eq!(attempt_rows(&pool, id).await, 0);
    let (attempts, due_in): (i32, f64) = sqlx::query_as(
        "SELECT attempts, EXTRACT(EPOCH FROM run_at - now())::float8 FROM jobs WHERE id = $1",
    )
    .bind(id.0)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts, 0, "a re-queue is not a try");
    assert!((50.0..=61.0).contains(&due_in), "ledger due in {due_in}s");
    // The storage row is re-fetchable: failed with tries left, due in about a minute.
    let (status, stored_attempts, max_attempts) = storage_row(&pool, id)
        .await
        .unwrap_or_else(|| unreachable!("storage row"));
    assert_eq!(status, "Failed");
    assert!(stored_attempts < max_attempts);
    let due_in: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM run_at - now())::float8 FROM apalis.jobs WHERE id = $1",
    )
    .bind(Ulid::from(id.0).to_string())
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert!((50.0..=61.0).contains(&due_in), "storage due in {due_in}s");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_type_runs_once_a_newer_worker_registers_it(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let newer = newer_registry(RetryPolicy::None, false, Arc::clone(&seen));
    let id = relayed_by_newer_worker(&pool, &newer).await;
    older_worker_requeues(&pool, id, 1).await;

    make_due(&pool, id).await;
    let worker = RunningWorker::start(pool.clone(), newer).await;
    let done = wait_until_async(Duration::from_secs(15), || async {
        state_of(&pool, id).await == "succeeded"
    })
    .await;
    worker.stop().await;

    assert!(done, "state {}", state_of(&pool, id).await);
    let runs = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        runs,
        vec![1],
        "one run, attempt 1: the requeue did not count"
    );
    assert_eq!(attempt_rows(&pool, id).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn requeue_keeps_the_retry_budget(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let policy = RetryPolicy::Backoff {
        max_attempts: Some(3),
        initial: Duration::from_millis(100),
        max: Duration::from_millis(100),
    };
    let newer = newer_registry(policy, true, Arc::clone(&seen));
    let id = relayed_by_newer_worker(&pool, &newer).await;
    older_worker_requeues(&pool, id, 2).await;

    make_due(&pool, id).await;
    let worker = RunningWorker::start(pool.clone(), newer).await;
    let finished = wait_until_async(Duration::from_secs(20), || async {
        state_of(&pool, id).await == "dead"
    })
    .await;
    // A fourth run would show up here.
    tokio::time::sleep(Duration::from_secs(1)).await;
    worker.stop().await;

    assert!(finished, "state {}", state_of(&pool, id).await);
    let runs = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(runs, vec![1, 2, 3], "three tries after two re-queues");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_type_aborts_after_max_age(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let newer = newer_registry(RetryPolicy::None, false, Arc::default());
    let id = relayed_by_newer_worker(&pool, &newer).await;
    sqlx::query("UPDATE jobs SET created_at = now() - interval '8 days' WHERE id = $1")
        .bind(id.0)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));

    let worker = RunningWorker::start(pool.clone(), JobRegistry::default()).await;
    let killed = wait_until_async(Duration::from_secs(15), || async {
        state_of(&pool, id).await == "killed"
    })
    .await;
    worker.stop().await;

    assert!(killed, "state {}", state_of(&pool, id).await);
    let (kind, code): (String, String) = sqlx::query_as(
        "SELECT error_kind, error_code FROM job_attempts WHERE job_id = $1 ORDER BY attempt",
    )
    .bind(id.0)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        (kind.as_str(), code.as_str()),
        ("unknown_type", "not_registered")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unregistered_outbox_row_killed_only_after_max_age(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let young = queue(&pool)
        .enqueue(&FutureJob {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));
    let old = queue(&pool)
        .enqueue(&FutureJob {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));
    for (id, days) in [(young, 6), (old, 8)] {
        sqlx::query("UPDATE jobs SET run_at = now() - make_interval(days => $2) WHERE id = $1")
            .bind(id.0)
            .bind(days)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    let none = JobRegistry::default().registered_types();
    run_job_history_purge(&pool, &jobs_settings(), &none, Utc::now()).await;

    assert_eq!(state_of(&pool, young).await, "queued");
    assert_eq!(state_of(&pool, old).await, "killed");
    let code: String = sqlx::query_scalar("SELECT error_code FROM job_attempts WHERE job_id = $1")
        .bind(old.0)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(code, "type_unregistered");
    let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
        .bind(young.0)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outbox, 1, "the young row stays in the outbox");
}

#[test]
fn publishing_queue_is_listed() {
    assert_eq!(Queue::Publishing.as_str(), "publishing");
    assert_eq!(Queue::ALL.len(), 4);
}
