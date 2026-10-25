use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use postit_core::SystemIdGenerator;
use postit_data::jobs::JobsRepo;
use postit_data::recurring_runs::RecurringRunsRepo;
use postit_jobs::testkit::{RunningWorker, finished_job_ids, wait_until, wait_until_async};
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, Job, JobConsole, JobContext, JobError, JobQueue, JobRegistry,
    Queue, RetryPolicy,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Probe {
    tag: u32,
}
impl Job for Probe {
    const JOB_TYPE: &'static str = "probe";
    const QUEUE: Queue = Queue::Default;
}

/// Each worker test holds ~10 database connections; running a dozen at once exceeds
/// Postgres's default `max_connections`, so tests that start a worker run one at a time.
static WORKERS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_writes_a_queued_ledger_row_with_the_queue_name(pool: PgPool) {
    let id = queue(&pool)
        .enqueue(&Probe { tag: 7 })
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (state, queue_name, job_type): (String, String, String) =
        sqlx::query_as("SELECT state, queue, job_type FROM jobs WHERE id = $1")
            .bind(id.0)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        (state.as_str(), queue_name.as_str(), job_type.as_str()),
        ("queued", "default", "probe")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_rolled_back_enqueue_leaves_no_ledger_row_and_no_outbox_row(pool: PgPool) {
    let q = queue(&pool);
    let mut tx = pool.begin().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = q
        .enqueue_in(&mut tx, &Probe { tag: 1 }, None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    tx.rollback().await.unwrap_or_else(|e| unreachable!("{e}"));
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE id = $1")
        .bind(id.0)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
        .bind(id.0)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((jobs, outbox), (0, 0));
}

async fn job_state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

async fn reaches_state(pool: &PgPool, id: Uuid, want: &str) -> bool {
    wait_until_async(Duration::from_secs(20), || async {
        job_state(pool, id).await == want
    })
    .await
}

type AttemptRow = (String, Option<String>, Option<String>, Option<String>);

/// `(outcome, error_kind, error_code, error_message)` per attempt, oldest first.
async fn attempts(pool: &PgPool, id: Uuid) -> Vec<AttemptRow> {
    sqlx::query_as(
        "SELECT outcome, error_kind, error_code, error_message FROM job_attempts
         WHERE job_id = $1 ORDER BY attempt",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn failing_registry(retry: RetryPolicy, error: fn() -> JobError) -> JobRegistry {
    let mut registry = JobRegistry::default();
    registry
        .register(
            retry,
            move |_: Probe, _: JobContext| async move { Err(error()) },
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    registry
}

async fn enqueue_probe(pool: &PgPool) -> Uuid {
    queue(pool)
        .enqueue(&Probe { tag: 1 })
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0
}

fn counting_registry(counter: &Arc<AtomicUsize>) -> JobRegistry {
    let mut registry = JobRegistry::default();
    let counter = Arc::clone(counter);
    registry
        .register(RetryPolicy::None, move |_: Probe, _: JobContext| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
    registry
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_successful_job_ends_succeeded_with_one_attempt(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Probe, _: JobContext| async {
            Ok(())
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "succeeded").await);
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_attempts WHERE job_id = $1 AND outcome = 'succeeded'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts, 1);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failing_job_with_retries_left_is_failed_then_dead_when_exhausted(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let registry = failing_registry(
        RetryPolicy::Backoff {
            max_attempts: Some(2),
            initial: Duration::from_millis(100),
            max: Duration::from_millis(100),
        },
        || JobError::Retry("secret@example.com".into()),
    );
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "dead").await);
    let rows = attempts(&pool, id).await;
    assert_eq!(
        rows,
        vec![
            (
                "retrying".to_string(),
                Some("retry".to_string()),
                None,
                None
            ),
            (
                "failed".to_string(),
                Some("retries_exhausted".to_string()),
                Some("max_attempts".to_string()),
                None
            ),
        ]
    );
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_job_that_fails_once_then_succeeds_ends_succeeded_with_two_attempts(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = JobRegistry::default();
    let seen = Arc::clone(&calls);
    registry
        .register(
            RetryPolicy::Backoff {
                max_attempts: Some(3),
                initial: Duration::from_millis(100),
                max: Duration::from_millis(100),
            },
            move |_: Probe, _: JobContext| {
                let seen = Arc::clone(&seen);
                async move {
                    if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(JobError::Retry("try again".into()))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "succeeded").await);
    let outcomes: Vec<String> = attempts(&pool, id)
        .await
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(outcomes, ["retrying", "succeeded"]);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_fatal_error_is_killed(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let registry = failing_registry(RetryPolicy::None, || JobError::Fatal("bad".into()));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "killed").await);
    let rows = attempts(&pool, id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "killed");
    assert_eq!(rows[0].1.as_deref(), Some("fatal"));
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_panicking_handler_is_killed_with_the_panic_kind(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Probe, _: JobContext| async {
            unreachable!("handler panics on purpose")
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "killed").await);
    let rows = attempts(&pool, id).await;
    assert_eq!(rows[0].1.as_deref(), Some("panic"));
    assert_eq!(rows[0].2.as_deref(), Some("handler_panic"));
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_opted_in_type_stores_its_error_message(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Probe>(ConsoleSpec::new().show_error_message());
    let registry = failing_registry(RetryPolicy::None, || JobError::Fatal("disk full".into()))
        .with_console(catalog);
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "killed").await);
    assert_eq!(attempts(&pool, id).await[0].3.as_deref(), Some("disk full"));
    worker.stop().await;
}

const SECRET_MESSAGE: &str = "smtp://mailer:hunter2@smtp.example.com rejected ada@example.com: \
     535 auth failed, Authorization: Bearer t0ken-abc, api_key=k-123";

fn console_for(pool: &PgPool, catalog: ConsoleCatalog) -> JobConsole {
    JobConsole::new(
        pool.clone(),
        queue(pool),
        catalog,
        Arc::new(postit_core::SystemClock),
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_opted_in_message_is_stored_and_returned_redacted(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Probe>(ConsoleSpec::new().show_error_message());
    let registry = failing_registry(RetryPolicy::None, || JobError::Fatal(SECRET_MESSAGE.into()))
        .with_console(catalog.clone());
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "killed").await);
    worker.stop().await;

    let stored = attempts(&pool, id).await[0].3.clone();
    let detail = console_for(&pool, catalog)
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let returned = detail.attempts[0].error_message.clone();
    assert_eq!(returned, stored);
    let message = returned.unwrap_or_else(|| unreachable!("an opted-in type returns its message"));
    assert!(message.contains("535 auth failed"), "{message}");
    for secret in ["hunter2", "t0ken-abc", "k-123", "ada@example.com"] {
        assert!(!message.contains(secret), "{secret} leaked: {message}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_message_of_a_type_that_did_not_opt_in_is_neither_stored_nor_returned(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Probe>(ConsoleSpec::new());
    let registry = failing_registry(RetryPolicy::None, || JobError::Fatal(SECRET_MESSAGE.into()))
        .with_console(catalog.clone());
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = enqueue_probe(&pool).await;
    assert!(reaches_state(&pool, id, "killed").await);
    worker.stop().await;

    assert_eq!(attempts(&pool, id).await[0].3, None);
    let detail = console_for(&pool, catalog)
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.attempts[0].error_message, None);
    assert!(!format!("{detail:?}").contains("ada@example.com"));
}

/// Cancels the queued job's ledger row but leaves its outbox row, so the relay still pushes
/// it into the storage.
async fn enqueue_and_cancel(pool: &PgPool) -> Uuid {
    let id = enqueue_probe(pool).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    id
}

async fn acked(pool: &PgPool, id: Uuid) -> bool {
    wait_until_async(Duration::from_secs(20), || async {
        finished_job_ids(pool).await.contains(&id)
    })
    .await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_cancelled_job_never_runs(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let id = enqueue_and_cancel(&pool).await;
    let runs = Arc::new(AtomicUsize::new(0));
    let worker = RunningWorker::start(pool.clone(), counting_registry(&runs)).await;
    assert!(acked(&pool, id).await);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(job_state(&pool, id).await, "cancelled");
    assert!(attempts(&pool, id).await.is_empty());
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_deleted_cancelled_job_never_runs(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let id = enqueue_and_cancel(&pool).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::delete_finished(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(conn);
    let runs = Arc::new(AtomicUsize::new(0));
    let worker = RunningWorker::start(pool.clone(), counting_registry(&runs)).await;
    assert!(acked(&pool, id).await);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    worker.stop().await;
}

/// Enqueues a probe due in a few seconds and waits until the relay has moved it from the
/// outbox into the job storage, so whatever the test does next happens after the relay.
async fn enqueue_relayed(pool: &PgPool) -> Uuid {
    let id = queue(pool)
        .enqueue_at(
            &Probe { tag: 1 },
            chrono::Utc::now() + chrono::Duration::seconds(3),
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    assert!(
        wait_until_async(Duration::from_secs(10), || async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap_or_else(|e| unreachable!("{e}"))
                == 0
        })
        .await
    );
    id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_job_cancelled_after_the_relay_never_runs(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let runs = Arc::new(AtomicUsize::new(0));
    let worker = RunningWorker::start(pool.clone(), counting_registry(&runs)).await;
    let id = enqueue_relayed(&pool).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(conn);
    assert!(acked(&pool, id).await);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(job_state(&pool, id).await, "cancelled");
    assert!(attempts(&pool, id).await.is_empty());
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_job_cancelled_then_purged_after_the_relay_never_runs(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let runs = Arc::new(AtomicUsize::new(0));
    let worker = RunningWorker::start(pool.clone(), counting_registry(&runs)).await;
    let id = enqueue_relayed(&pool).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    let now = chrono::Utc::now() + chrono::Duration::seconds(1);
    assert_eq!(
        JobsRepo::purge_finished(&mut conn, now, now)
            .await
            .unwrap_or_else(|e| unreachable!("{e}")),
        1
    );
    drop(conn);
    assert!(acked(&pool, id).await);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_ledger_write_failure_does_not_stop_the_job(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    sqlx::query("DROP TABLE job_attempts")
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let runs = Arc::new(AtomicUsize::new(0));
    let worker = RunningWorker::start(pool.clone(), counting_registry(&runs)).await;
    enqueue_probe(&pool).await;
    assert!(wait_until(Duration::from_secs(20), || runs.load(Ordering::SeqCst) == 1).await);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_worker_run_job_leaves_no_open_attempt(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |p: Probe, _: JobContext| async move {
            if p.tag == 1 {
                Ok(())
            } else {
                Err(JobError::Fatal("no".into()))
            }
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let ok = queue(&pool)
        .enqueue(&Probe { tag: 1 })
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    let bad = queue(&pool)
        .enqueue(&Probe { tag: 2 })
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    assert!(reaches_state(&pool, ok, "succeeded").await);
    assert!(reaches_state(&pool, bad, "killed").await);
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE finished_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(open, 0);
    worker.stop().await;
}

#[derive(Serialize, Deserialize, Default)]
struct Tick {
    run_id: Uuid,
}
impl Job for Tick {
    const JOB_TYPE: &'static str = "tick";
    const QUEUE: Queue = Queue::Maintenance;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_cancelled_recurring_run_is_recorded_failed(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = JobRegistry::default();
    let counter = Arc::clone(&runs);
    registry
        .register_recurring(
            "0 0 0 1 1 *",
            RetryPolicy::None,
            move |_: Tick, _: JobContext| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    let run_id = Uuid::now_v7();
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    RecurringRunsRepo::insert_manual(&mut conn, run_id, "tick", chrono::Utc::now())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let id = queue(&pool)
        .enqueue_in(&mut conn, &Tick { run_id }, None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    assert!(
        JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(conn);
    let worker = RunningWorker::start(pool.clone(), registry).await;
    assert!(acked(&pool, id).await);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM job_recurring_runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outcome.as_deref(), Some("failed"));
    worker.stop().await;
}
