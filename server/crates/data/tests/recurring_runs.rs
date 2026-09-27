use chrono::{DateTime, Duration, TimeZone, Utc};
use postit_data::recurring_runs::{RecurringRunsRepo, RunOutcome};
use sqlx::PgPool;
use uuid::Uuid;

fn tick() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 27, 3, 20, 0)
        .single()
        .unwrap_or_else(|| unreachable!("valid timestamp"))
}

#[sqlx::test]
async fn only_the_first_scheduled_insert_for_a_tick_wins(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let first = RecurringRunsRepo::try_insert_scheduled(&mut conn, Uuid::now_v7(), "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("first: {e}"));
    let second =
        RecurringRunsRepo::try_insert_scheduled(&mut conn, Uuid::now_v7(), "purge", tick())
            .await
            .unwrap_or_else(|e| unreachable!("second: {e}"));
    let other_name =
        RecurringRunsRepo::try_insert_scheduled(&mut conn, Uuid::now_v7(), "audit", tick())
            .await
            .unwrap_or_else(|e| unreachable!("other name: {e}"));

    assert!(first);
    assert!(!second);
    assert!(other_name);
}

#[sqlx::test]
async fn manual_runs_never_conflict(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    RecurringRunsRepo::try_insert_scheduled(&mut conn, Uuid::now_v7(), "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("scheduled: {e}"));
    RecurringRunsRepo::insert_manual(&mut conn, Uuid::now_v7(), "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("manual 1: {e}"));
    RecurringRunsRepo::insert_manual(&mut conn, Uuid::now_v7(), "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("manual 2: {e}"));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_recurring_runs")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 3);
}

#[sqlx::test]
async fn set_job_id_and_finish_record_the_outcome(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = Uuid::now_v7();
    let job_id = Uuid::now_v7();
    RecurringRunsRepo::try_insert_scheduled(&mut conn, id, "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    RecurringRunsRepo::set_job_id(&mut conn, id, job_id)
        .await
        .unwrap_or_else(|e| unreachable!("set_job_id: {e}"));
    RecurringRunsRepo::finish(
        &mut conn,
        id,
        RunOutcome::Succeeded,
        tick() + Duration::seconds(3),
    )
    .await
    .unwrap_or_else(|e| unreachable!("finish: {e}"));

    let (stored_job_id, outcome): (Option<Uuid>, Option<String>) =
        sqlx::query_as("SELECT job_id, outcome FROM job_recurring_runs WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert_eq!(stored_job_id, Some(job_id));
    assert_eq!(outcome.as_deref(), Some("succeeded"));
}

#[sqlx::test]
async fn purge_deletes_only_rows_older_than_the_cutoff(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let old = Uuid::now_v7();
    let recent = Uuid::now_v7();
    RecurringRunsRepo::insert_manual(&mut conn, old, "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("old: {e}"));
    RecurringRunsRepo::insert_manual(&mut conn, recent, "purge", tick())
        .await
        .unwrap_or_else(|e| unreachable!("recent: {e}"));
    sqlx::query(
        "UPDATE job_recurring_runs SET created_at = now() - interval '40 days' WHERE id = $1",
    )
    .bind(old)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("age row: {e}"));

    let deleted = RecurringRunsRepo::purge_older_than(&mut conn, Utc::now() - Duration::days(30))
        .await
        .unwrap_or_else(|e| unreachable!("purge: {e}"));
    assert_eq!(deleted, 1);
}
