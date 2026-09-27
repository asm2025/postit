use std::sync::Arc;

use chrono::{Duration, Utc};
use postit_core::SystemIdGenerator;
use postit_jobs::{Job, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Probe {
    user_id: uuid::Uuid,
}

impl Job for Probe {
    const JOB_TYPE: &'static str = "probe";
    const QUEUE: Queue = Queue::Default;
}

fn queue(pool: PgPool) -> JobQueue {
    JobQueue::new(pool, Arc::new(SystemIdGenerator))
}

async fn outbox_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_in_a_rolled_back_transaction_leaves_nothing(pool: PgPool) {
    let q = queue(pool.clone());
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    q.enqueue_in(
        &mut tx,
        &Probe {
            user_id: uuid::Uuid::now_v7(),
        },
        None,
    )
    .await
    .unwrap_or_else(|e| unreachable!("enqueue_in: {e}"));
    tx.rollback()
        .await
        .unwrap_or_else(|e| unreachable!("rollback: {e}"));

    assert_eq!(outbox_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_in_a_committed_transaction_writes_type_payload_and_id(pool: PgPool) {
    let q = queue(pool.clone());
    let user_id = uuid::Uuid::now_v7();
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let job_id = q
        .enqueue_in(&mut tx, &Probe { user_id }, None)
        .await
        .unwrap_or_else(|e| unreachable!("enqueue_in: {e}"));
    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));

    let (id, job_type, payload): (uuid::Uuid, String, serde_json::Value) =
        sqlx::query_as("SELECT id, job_type, payload FROM job_outbox")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert_eq!(id, job_id.0);
    assert_eq!(job_type, "probe");
    assert_eq!(payload, serde_json::json!({ "user_id": user_id }));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_at_stores_run_at(pool: PgPool) {
    let at = Utc::now() + Duration::hours(2);
    queue(pool.clone())
        .enqueue_at(
            &Probe {
                user_id: uuid::Uuid::now_v7(),
            },
            at,
        )
        .await
        .unwrap_or_else(|e| unreachable!("enqueue_at: {e}"));

    let run_at: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT run_at FROM job_outbox")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert!((run_at - at).num_milliseconds().abs() < 1);
}
