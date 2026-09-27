use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use postit_core::SystemIdGenerator;
use postit_jobs::testkit::{RunningWorker, jobs_settings, wait_until};
use postit_jobs::{Job, JobContext, JobError, JobQueue, JobRegistry, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Probe {
    tag: u32,
}
impl Job for Probe {
    const JOB_TYPE: &'static str = "probe";
    const QUEUE: Queue = Queue::Default;
}

#[derive(Serialize, Deserialize)]
struct SlowMail {}
impl Job for SlowMail {
    const JOB_TYPE: &'static str = "slow_mail";
    const QUEUE: Queue = Queue::Mail;
}

#[derive(Serialize, Deserialize)]
struct Chore {}
impl Job for Chore {
    const JOB_TYPE: &'static str = "chore";
    const QUEUE: Queue = Queue::Maintenance;
}

#[derive(Serialize, Deserialize)]
struct Flaky {}
impl Job for Flaky {
    const JOB_TYPE: &'static str = "flaky";
    const QUEUE: Queue = Queue::Default;
}

fn queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator))
}

fn probe_registry(seen: Arc<Mutex<Vec<u32>>>) -> JobRegistry {
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, move |job: Probe, _ctx: JobContext| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(job.tag);
                Ok(())
            }
        })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    registry
}

fn seen_tags(seen: &Arc<Mutex<Vec<u32>>>) -> Vec<u32> {
    seen.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_job_enqueued_through_job_queue_runs_in_a_worker(pool: PgPool) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let worker = RunningWorker::start(pool.clone(), probe_registry(Arc::clone(&seen))).await;

    queue(&pool)
        .enqueue(&Probe { tag: 7 })
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));

    assert!(wait_until(Duration::from_secs(10), || seen_tags(&seen) == vec![7]).await);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_rolled_back_enqueue_in_never_runs(pool: PgPool) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let worker = RunningWorker::start(pool.clone(), probe_registry(Arc::clone(&seen))).await;
    let q = queue(&pool);

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    q.enqueue_in(&mut tx, &Probe { tag: 1 }, None)
        .await
        .unwrap_or_else(|e| unreachable!("enqueue_in: {e}"));
    tx.rollback()
        .await
        .unwrap_or_else(|e| unreachable!("rollback: {e}"));
    q.enqueue(&Probe { tag: 2 })
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));

    assert!(wait_until(Duration::from_secs(10), || seen_tags(&seen).contains(&2)).await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(seen_tags(&seen), vec![2]);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_at_does_not_run_before_run_at(pool: PgPool) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let worker = RunningWorker::start(pool.clone(), probe_registry(Arc::clone(&seen))).await;
    let started = Instant::now();

    queue(&pool)
        .enqueue_at(&Probe { tag: 3 }, Utc::now() + chrono::Duration::seconds(2))
        .await
        .unwrap_or_else(|e| unreachable!("enqueue_at: {e}"));

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(seen_tags(&seen).is_empty());
    assert!(wait_until(Duration::from_secs(15), || seen_tags(&seen) == vec![3]).await);
    assert!(started.elapsed() >= Duration::from_secs(2));
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unregistered_job_type_stays_in_the_outbox(pool: PgPool) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let worker = RunningWorker::start(pool.clone(), probe_registry(Arc::clone(&seen))).await;
    sqlx::query(
        "INSERT INTO job_outbox (id, job_type, payload, run_at) VALUES ($1, 'future_job', '{}', now())",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    queue(&pool)
        .enqueue(&Probe { tag: 4 })
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));

    assert!(wait_until(Duration::from_secs(10), || seen_tags(&seen) == vec![4]).await);
    let left: Vec<String> = sqlx::query_scalar("SELECT job_type FROM job_outbox")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert_eq!(left, vec!["future_job".to_string()]);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_blocked_mail_queue_does_not_delay_maintenance(pool: PgPool) {
    let release = Arc::new(tokio::sync::Notify::new());
    let chores = Arc::new(AtomicUsize::new(0));
    let mut registry = JobRegistry::default();
    let gate = Arc::clone(&release);
    registry
        .register(RetryPolicy::None, move |_: SlowMail, _| {
            let gate = Arc::clone(&gate);
            async move {
                gate.notified().await;
                Ok(())
            }
        })
        .unwrap_or_else(|e| unreachable!("register slow: {e}"));
    let counter = Arc::clone(&chores);
    registry
        .register(RetryPolicy::None, move |_: Chore, _| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap_or_else(|e| unreachable!("register chore: {e}"));
    let mut settings = jobs_settings();
    settings.concurrency.insert("mail".into(), 1);
    let worker = RunningWorker::start_with(pool.clone(), settings, registry).await;
    let q = queue(&pool);

    for _ in 0..3 {
        q.enqueue(&SlowMail {})
            .await
            .unwrap_or_else(|e| unreachable!("enqueue mail: {e}"));
    }
    q.enqueue(&Chore {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue chore: {e}"));

    assert!(
        wait_until(Duration::from_secs(10), || chores.load(Ordering::SeqCst)
            == 1)
        .await
    );
    release.notify_waiters();
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retries_follow_the_policy_and_report_the_last_attempt(pool: PgPool) {
    let attempts = Arc::new(Mutex::new(Vec::<(u32, bool, Instant)>::new()));
    let mut registry = JobRegistry::default();
    let record = Arc::clone(&attempts);
    registry
        .register(
            RetryPolicy::Backoff {
                max_attempts: Some(3),
                initial: Duration::from_millis(600),
                max: Duration::from_millis(900),
            },
            move |_: Flaky, ctx: JobContext| {
                let record = Arc::clone(&record);
                async move {
                    record
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((ctx.attempt, ctx.is_last_attempt(), Instant::now()));
                    Err(JobError::Retry("always".into()))
                }
            },
        )
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;

    queue(&pool)
        .enqueue(&Flaky {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));

    let snapshot = || {
        attempts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    };
    assert!(wait_until(Duration::from_secs(20), || snapshot().len() == 3).await);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let runs = snapshot();
    let flags: Vec<(u32, bool)> = runs.iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(flags, vec![(1, false), (2, false), (3, true)]);
    // The backoff (600 ms, then 900 ms) is honoured between attempts.
    assert!(runs[1].2 - runs[0].2 >= Duration::from_millis(600));
    assert!(runs[2].2 - runs[1].2 >= Duration::from_millis(900));
    worker.stop().await;
}
