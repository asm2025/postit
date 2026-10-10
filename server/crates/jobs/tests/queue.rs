use std::sync::Arc;

use chrono::{Duration, Utc};
use postit_core::{IdGenerator, SystemIdGenerator};
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
    JobQueue::new(
        pool,
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    )
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

/// Hands out one fixed ID, so a test can make the ledger insert collide with a row it seeded.
struct FixedId(uuid::Uuid);

impl IdGenerator for FixedId {
    fn generate(&self) -> uuid::Uuid {
        self.0
    }
}

/// `xmin` is the ID of the transaction that wrote a row version. Every statement on an
/// autocommit connection gets its own transaction ID, while every write inside one
/// transaction (no savepoints here) shares the top-level ID. So equal `xmin`s prove both
/// rows were written by one transaction, whatever the timing.
#[sqlx::test(migrations = "../data/migrations")]
async fn enqueue_writes_the_outbox_and_ledger_rows_in_one_transaction(pool: PgPool) {
    let id = queue(pool.clone())
        .enqueue(&Probe {
            user_id: uuid::Uuid::now_v7(),
        })
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"));
    let (outbox_xmin, ledger_xmin): (String, String) = sqlx::query_as(
        "SELECT (SELECT xmin::text FROM job_outbox WHERE id = $1),
                (SELECT xmin::text FROM jobs WHERE id = $1)",
    )
    .bind(id.0)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("xmin: {e}"));
    assert_eq!(outbox_xmin, ledger_xmin);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failed_ledger_insert_leaves_no_outbox_row(pool: PgPool) {
    let id = uuid::Uuid::now_v7();
    // A ledger row already holds the ID, so `enqueue`'s ledger insert (its second write)
    // fails after the outbox insert succeeded.
    sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, run_at) VALUES ($1, 'x', 'default', '{}', now())")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("seed: {e}"));
    let result = JobQueue::new(
        pool.clone(),
        Arc::new(FixedId(id)),
        Arc::new(postit_core::SystemClock),
    )
    .enqueue(&Probe {
        user_id: uuid::Uuid::now_v7(),
    })
    .await;
    assert!(result.is_err());
    assert_eq!(outbox_count(&pool).await, 0);
}
