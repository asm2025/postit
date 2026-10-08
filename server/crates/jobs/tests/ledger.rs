use std::sync::Arc;

use postit_core::SystemIdGenerator;
use postit_jobs::{Job, JobQueue, Queue};
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

fn queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator))
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
