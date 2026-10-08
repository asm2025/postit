use chrono::{Duration, Utc};
use postit_data::jobs::{
    AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobState, JobsRepo, NewJob,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn new_job(id: Uuid, payload: &serde_json::Value) -> NewJob<'_> {
    NewJob {
        id,
        job_type: "send_email",
        queue: "mail",
        payload,
        run_at: Utc::now(),
        recurring_name: None,
        retried_from: None,
    }
}

async fn state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("state: {e}"))
}

#[sqlx::test]
async fn a_new_job_is_queued_with_no_attempts(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    JobsRepo::insert(&mut conn, &new_job(id, &json!({"recipient": "x"})))
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    assert_eq!(state(&pool, id).await, "queued");
}

#[sqlx::test]
async fn begin_then_finish_records_one_attempt(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w1", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    assert!(matches!(begin, Begin::Run));
    assert_eq!(state(&pool, id).await, "running");
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish {
            outcome: AttemptOutcome::Succeeded,
            state: JobState::Succeeded,
            next_run_at: None,
            error: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("finish: {e}"));
    assert_eq!(state(&pool, id).await, "succeeded");
    let finished: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_attempts WHERE job_id = $1 AND finished_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(finished, 1);
}

#[sqlx::test]
async fn a_cancelled_job_is_not_run(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(begin, Begin::Cancelled));
}

#[sqlx::test]
async fn cancel_is_refused_while_running(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        !JobsRepo::cancel(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
}

#[sqlx::test]
async fn a_new_try_closes_an_open_attempt_as_interrupted(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    // The worker dies; the storage re-fetches the task as attempt 2.
    JobsRepo::begin_attempt(&mut conn, id, 2, "w2", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outcome: String =
        sqlx::query_scalar("SELECT outcome FROM job_attempts WHERE job_id = $1 AND attempt = 1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outcome, "interrupted");
}

#[sqlx::test]
async fn a_missing_ledger_row_is_missing_not_an_error(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let begin = JobsRepo::begin_attempt(&mut conn, Uuid::now_v7(), 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(begin, Begin::Missing));
}

#[sqlx::test]
async fn a_redelivered_finished_job_is_not_run_again(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish {
            outcome: AttemptOutcome::Succeeded,
            state: JobState::Succeeded,
            next_run_at: None,
            error: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    // The storage ack was lost; the task comes back as attempt 2.
    let begin = JobsRepo::begin_attempt(&mut conn, id, 2, "w2", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(begin, Begin::Finished));
    assert_eq!(state(&pool, id).await, "succeeded");
}

#[sqlx::test]
async fn a_retry_waiting_job_is_failed_not_finished_and_stores_no_message_by_default(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let next = Utc::now() + Duration::seconds(60);
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish {
            outcome: AttemptOutcome::Retrying,
            state: JobState::Failed,
            next_run_at: Some(next),
            error: Some(AttemptError {
                kind: ErrorKind::Retry,
                code: None,
                message: None,
            }),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let message: Option<String> =
        sqlx::query_scalar("SELECT error_message FROM job_attempts WHERE job_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(message, None);
    assert_eq!(state(&pool, id).await, "failed");
    let finished: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT finished_at FROM jobs WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(finished, None);
}

#[sqlx::test]
async fn delete_finished_refuses_a_live_job(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        !JobsRepo::delete_finished(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    JobsRepo::cancel(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        JobsRepo::delete_finished(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
}
