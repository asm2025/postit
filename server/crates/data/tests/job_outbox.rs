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
