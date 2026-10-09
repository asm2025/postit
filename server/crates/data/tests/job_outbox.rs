use chrono::{Duration, Utc};
use postit_data::job_outbox::JobOutboxRepo;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn insert_then_claim_then_delete(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = Uuid::now_v7();
    let run_at = Utc::now();
    JobOutboxRepo::insert(&mut conn, id, "probe", &json!({"user_id": "x"}), run_at)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let rows = JobOutboxRepo::claim_batch(&mut tx, &["probe".to_string()], &[], 10)
        .await
        .unwrap_or_else(|e| unreachable!("claim: {e}"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].job_type, "probe");
    assert_eq!(rows[0].payload, json!({"user_id": "x"}));

    JobOutboxRepo::delete(&mut tx, id)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(remaining, 0);
}

#[sqlx::test]
async fn claim_skips_rows_locked_by_another_transaction(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    for _ in 0..2 {
        JobOutboxRepo::insert(&mut conn, Uuid::now_v7(), "probe", &json!({}), Utc::now())
            .await
            .unwrap_or_else(|e| unreachable!("insert: {e}"));
    }

    let mut first = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let first_rows = JobOutboxRepo::claim_batch(&mut first, &["probe".to_string()], &[], 1)
        .await
        .unwrap_or_else(|e| unreachable!("claim 1: {e}"));
    let mut second = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let second_rows = JobOutboxRepo::claim_batch(&mut second, &["probe".to_string()], &[], 10)
        .await
        .unwrap_or_else(|e| unreachable!("claim 2: {e}"));

    assert_eq!(first_rows.len(), 1);
    assert_eq!(second_rows.len(), 1);
    assert_ne!(first_rows[0].id, second_rows[0].id);
}

#[sqlx::test]
async fn claim_ignores_unlisted_job_types(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    JobOutboxRepo::insert(
        &mut conn,
        Uuid::now_v7(),
        "future_job",
        &json!({}),
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let rows = JobOutboxRepo::claim_batch(&mut tx, &["probe".to_string()], &[], 10)
        .await
        .unwrap_or_else(|e| unreachable!("claim: {e}"));
    assert!(rows.is_empty());
}

#[sqlx::test]
async fn claim_skips_excluded_ids(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let ids = [Uuid::now_v7(), Uuid::now_v7()];
    for id in ids {
        JobOutboxRepo::insert(&mut conn, id, "probe", &json!({}), Utc::now())
            .await
            .unwrap_or_else(|e| unreachable!("insert: {e}"));
    }
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let rows = JobOutboxRepo::claim_batch(&mut tx, &["probe".to_string()], &[ids[0]], 10)
        .await
        .unwrap_or_else(|e| unreachable!("claim: {e}"));
    let claimed: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    assert_eq!(claimed, vec![ids[1]]);
}

#[sqlx::test]
async fn rolled_back_insert_leaves_nothing(pool: PgPool) {
    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    JobOutboxRepo::insert(
        &mut tx,
        Uuid::now_v7(),
        "probe",
        &json!({}),
        Utc::now() + Duration::hours(1),
    )
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));
    tx.rollback()
        .await
        .unwrap_or_else(|e| unreachable!("rollback: {e}"));

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(remaining, 0);
}

#[sqlx::test]
async fn committed_insert_notifies_the_outbox_channel(pool: PgPool) {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("listener: {e}"));
    listener
        .listen(postit_data::job_outbox::OUTBOX_CHANNEL)
        .await
        .unwrap_or_else(|e| unreachable!("listen: {e}"));

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    JobOutboxRepo::insert(&mut conn, Uuid::now_v7(), "probe", &json!({}), Utc::now())
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
        .await
        .unwrap_or_else(|e| unreachable!("no notification within 5s: {e}"))
        .unwrap_or_else(|e| unreachable!("recv: {e}"));
    assert_eq!(notification.channel(), "postit_job_outbox");
}

/// A relayable job: a ledger row and its outbox row, due `hours_ago` hours ago.
async fn queued_with_outbox(pool: &PgPool, job_type: &str, hours_ago: i32) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, queue, payload, state, run_at)
         VALUES ($1, $2, 'default', '{}', 'queued', now() - make_interval(hours => $3))",
    )
    .bind(id)
    .bind(job_type)
    .bind(hours_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("job: {e}"));
    sqlx::query(
        "INSERT INTO job_outbox (id, job_type, payload, run_at)
         VALUES ($1, $2, '{}', now() - make_interval(hours => $3))",
    )
    .bind(id)
    .bind(job_type)
    .bind(hours_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    id
}

async fn outbox_rows(pool: &PgPool, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[sqlx::test]
async fn an_old_unregistered_outbox_job_is_killed_and_its_outbox_row_removed(pool: PgPool) {
    let old_unregistered = queued_with_outbox(&pool, "removed_type", 30).await;
    let recent_unregistered = queued_with_outbox(&pool, "removed_type", 1).await;
    let old_registered = queued_with_outbox(&pool, "probe", 30).await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));

    let killed = JobOutboxRepo::kill_unregistered(
        &mut conn,
        &["probe".to_string()],
        Utc::now() - Duration::hours(24),
    )
    .await
    .unwrap_or_else(|e| unreachable!("kill: {e}"));

    assert_eq!(killed, 1);
    assert_eq!(outbox_rows(&pool, old_unregistered).await, 0);
    let row: (String, i32, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT state, attempts, finished_at FROM jobs WHERE id = $1")
            .bind(old_unregistered)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("row: {e}"));
    assert_eq!((row.0.as_str(), row.1), ("killed", 1));
    assert!(
        row.2.is_some(),
        "a killed job ages out by the failed retention"
    );
    let attempts: Vec<(i32, String, String, String, String)> = sqlx::query_as(
        "SELECT attempt, worker, outcome, error_kind, error_code FROM job_attempts WHERE job_id = $1",
    )
    .bind(old_unregistered)
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("attempts: {e}"));
    assert_eq!(
        attempts,
        vec![(
            1,
            "ledger_sweep".into(),
            "interrupted".into(),
            "interrupted".into(),
            "type_unregistered".into()
        )]
    );
    for untouched in [recent_unregistered, old_registered] {
        assert_eq!(outbox_rows(&pool, untouched).await, 1);
        let state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
            .bind(untouched)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("state: {e}"));
        assert_eq!(state, "queued");
    }
}
