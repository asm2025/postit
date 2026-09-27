use std::sync::Arc;
use std::time::Duration;

use postit_core::SystemIdGenerator;
use postit_jobs::maintenance::{self, DataRetention, JobHistoryPurge};
use postit_jobs::testkit::{RunningWorker, jobs_settings};
use postit_jobs::{Job, JobContext, JobQueue, JobRegistry, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Noop {}
impl Job for Noop {
    const JOB_TYPE: &'static str = "noop";
    const QUEUE: Queue = Queue::Default;
}

#[test]
fn registration_adds_both_maintenance_job_types() {
    // sqlx 0.9's pool needs a Tokio context even to build a lazy (not-yet-connecting) pool.
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| unreachable!("runtime: {e}"));
    let _guard = runtime.enter();

    let settings = jobs_settings();
    let pool = PgPool::connect_lazy("postgres://localhost/unused")
        .unwrap_or_else(|e| unreachable!("lazy pool: {e}"));
    let mut registry = JobRegistry::default();
    maintenance::register_job_history_purge(&mut registry, pool.clone(), &settings)
        .unwrap_or_else(|e| unreachable!("history: {e}"));
    maintenance::register_data_retention(&mut registry, pool, &settings)
        .unwrap_or_else(|e| unreachable!("retention: {e}"));
    assert_eq!(
        registry.job_types(),
        vec![
            "data_retention".to_string(),
            "job_history_purge".to_string()
        ]
    );
    assert_eq!(JobHistoryPurge::JOB_TYPE, "job_history_purge");
    assert_eq!(DataRetention::JOB_TYPE, "data_retention");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_deletes_only_finished_jobs_past_their_window(pool: PgPool) {
    // Run two jobs to completion.
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Noop, _: JobContext| async { Ok(()) })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let q = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let old = q
        .enqueue(&Noop {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue old: {e}"));
    let recent = q
        .enqueue(&Noop {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue recent: {e}"));
    let done = || async { postit_jobs::testkit::finished_job_ids(&pool).await };
    let mut finished = Vec::new();
    for _ in 0..100 {
        finished = done().await;
        if finished.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    worker.stop().await;
    assert_eq!(finished.len(), 2);

    // Age one of them past the 7-day succeeded window.
    postit_jobs::testkit::age_finished_job(&pool, old.0, chrono::Duration::days(8)).await;
    // And an old recurring-run row (past the longest window) plus a recent one.
    sqlx::query(
        "INSERT INTO job_recurring_runs (id, name, scheduled_for, manual, created_at)
                 VALUES ($1, 'x', now(), TRUE, now() - interval '40 days')",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert run: {e}"));
    let recent_run = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO job_recurring_runs (id, name, scheduled_for, manual, created_at)
                 VALUES ($1, 'x', now(), TRUE, now() - interval '1 day')",
    )
    .bind(recent_run)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert recent run: {e}"));

    postit_jobs::testkit::run_job_history_purge(&pool, &jobs_settings()).await;

    let remaining = done().await;
    assert_eq!(remaining, vec![recent.0]);
    let runs: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM job_recurring_runs")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("runs: {e}"));
    assert_eq!(runs, vec![recent_run]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn data_retention_purges_expired_idempotency_keys(pool: PgPool) {
    let user = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
                 VALUES ($1, 'https://i', 's', 'n', 'member', 'active')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("user: {e}"));
    sqlx::query(
        "INSERT INTO idempotency_keys (id, owner_id, actor_id, key, route, request_hash, state, expires_at)
                 VALUES ($1, $2, $2, 'k', 'r', 'h', 'completed', now() - interval '1 hour')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("key: {e}"));

    postit_jobs::testkit::run_data_retention(&pool).await;

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM idempotency_keys")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(left, 0);
}
