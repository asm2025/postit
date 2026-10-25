use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use postit_jobs::testkit::RunningWorker;
use postit_jobs::{Job, JobContext, JobError, JobRegistry, JobsError, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize, Default)]
struct Tick {}
impl Job for Tick {
    const JOB_TYPE: &'static str = "tick";
    const QUEUE: Queue = Queue::Maintenance;
}

fn registry(counter: Arc<AtomicUsize>) -> JobRegistry {
    let mut registry = JobRegistry::default();
    registry
        .register_recurring(
            "* * * * * *",
            RetryPolicy::None,
            move |_: Tick, _: JobContext| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok::<(), JobError>(())
                }
            },
        )
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    registry
}

#[test]
fn an_invalid_schedule_is_rejected_at_registration() {
    let mut registry = JobRegistry::default();
    let result = registry.register_recurring("not a cron", RetryPolicy::None, |_: Tick, _| async {
        Ok(())
    });
    assert!(matches!(
        result,
        Err(JobsError::InvalidSchedule { name: "tick", .. })
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_workers_run_each_tick_exactly_once(pool: PgPool) {
    let runs = Arc::new(AtomicUsize::new(0));
    let first = RunningWorker::start(pool.clone(), registry(Arc::clone(&runs))).await;
    let second = RunningWorker::start(pool.clone(), registry(Arc::clone(&runs))).await;

    tokio::time::sleep(Duration::from_millis(4_500)).await;
    first.stop().await;
    second.stop().await;

    let count = |sql: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| unreachable!("{sql}: {e}"))
        }
    };
    let rows =
        count("SELECT COUNT(*) FROM job_recurring_runs WHERE name = 'tick' AND NOT manual").await;
    let succeeded = count(
        "SELECT COUNT(*) FROM job_recurring_runs WHERE name = 'tick' AND outcome = 'succeeded'",
    )
    .await;
    let duplicate_ticks = count(
        "SELECT COUNT(*) FROM (SELECT scheduled_for FROM job_recurring_runs
                               WHERE name = 'tick' AND NOT manual
                               GROUP BY scheduled_for HAVING COUNT(*) > 1) d",
    )
    .await;
    let handler_runs = i64::try_from(runs.load(Ordering::SeqCst)).unwrap_or(i64::MAX);

    assert!(rows >= 3, "expected at least 3 ticks in 4.5 s, got {rows}");
    assert_eq!(duplicate_ticks, 0, "a tick was claimed twice");
    // Each claimed tick ran at most once and recorded its outcome; the last one or two
    // may have been claimed but not run before the workers stopped.
    assert_eq!(handler_runs, succeeded);
    assert!(handler_runs <= rows);
    assert!(
        handler_runs >= rows - 2,
        "runs {handler_runs} vs ticks {rows}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failed_recurring_run_records_failed(pool: PgPool) {
    let mut registry = JobRegistry::default();
    registry
        .register_recurring("* * * * * *", RetryPolicy::None, |_: Tick, _| async {
            Err(JobError::Fatal("boom".into()))
        })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;

    let failed = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM job_recurring_runs WHERE name = 'tick' AND outcome = 'failed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(0)
    };
    let mut seen = 0;
    for _ in 0..50 {
        seen = failed().await;
        if seen > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    worker.stop().await;
    assert!(seen > 0);
}
