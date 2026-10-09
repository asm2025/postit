use chrono::{Duration, Utc};
use emixdb::dto::Pagination;
use postit_data::jobs::{
    AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobFilter, JobState, JobsRepo, NewJob,
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

async fn seed(pool: &PgPool, job_type: &str, run_at_offset_secs: i64) -> Uuid {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    let job = NewJob {
        id,
        job_type,
        queue: "mail",
        payload: &payload,
        run_at: Utc::now() + Duration::seconds(run_at_offset_secs),
        recurring_name: None,
        retried_from: None,
    };
    JobsRepo::insert(&mut conn, &job)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    id
}

async fn succeed(conn: &mut sqlx::PgConnection, id: Uuid) {
    JobsRepo::begin_attempt(conn, id, 1, "w", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(
        conn,
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
}

#[sqlx::test]
async fn scheduled_and_queued_are_split_by_run_at(pool: PgPool) {
    seed(&pool, "send_email", -5).await;
    seed(&pool, "send_email", 3600).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let page = Pagination {
        page: 1,
        page_size: 20,
    };
    let queued = JobsRepo::list(
        &mut conn,
        &JobFilter {
            state: Some("queued".into()),
            ..JobFilter::default()
        },
        page,
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let scheduled = JobsRepo::list(
        &mut conn,
        &JobFilter {
            state: Some("scheduled".into()),
            ..JobFilter::default()
        },
        page,
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((queued.total, scheduled.total), (1, 1));
}

#[sqlx::test]
async fn list_filters_by_type_and_paginates(pool: PgPool) {
    for _ in 0..3 {
        seed(&pool, "send_email", -1).await;
    }
    seed(&pool, "delete_user", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let filter = JobFilter {
        job_type: Some("send_email".into()),
        ..JobFilter::default()
    };
    let page = JobsRepo::list(
        &mut conn,
        &filter,
        Pagination {
            page: 2,
            page_size: 2,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(page.total, 3);
    assert_eq!(page.data.len(), 1);
}

#[sqlx::test]
async fn stats_count_states_types_queue_depth_and_throughput(pool: PgPool) {
    let done = seed(&pool, "send_email", -1).await;
    seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    succeed(&mut conn, done).await;
    let stats = JobsRepo::stats(&mut conn, Utc::now() - Duration::hours(24), 3600)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let count = |s: &str| {
        stats
            .states
            .iter()
            .find(|c| c.state == s)
            .map_or(0, |c| c.count)
    };
    assert_eq!((count("queued"), count("succeeded")), (1, 1));
    assert_eq!(
        stats
            .queues
            .iter()
            .find(|q| q.queue == "mail")
            .map(|q| q.count),
        Some(1)
    );
    assert_eq!(
        stats
            .types
            .iter()
            .find(|t| t.job_type == "send_email")
            .map(|t| t.count),
        Some(2)
    );
    assert_eq!(stats.throughput.iter().map(|b| b.succeeded).sum::<i64>(), 1);
}

#[sqlx::test]
async fn detail_returns_attempts_in_order_and_the_retry_link(pool: PgPool) {
    let id = seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    for n in 1..=2 {
        JobsRepo::begin_attempt(&mut conn, id, n, "w", None)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::finish_attempt(
            &mut conn,
            id,
            n,
            &Finish {
                outcome: AttemptOutcome::Retrying,
                state: JobState::Failed,
                next_run_at: Some(Utc::now()),
                error: Some(AttemptError {
                    kind: ErrorKind::Retry,
                    code: None,
                    message: None,
                }),
            },
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let attempts = JobsRepo::attempts(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        attempts.iter().map(|a| a.attempt).collect::<Vec<_>>(),
        vec![1, 2]
    );
    let child = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(
        &mut conn,
        &NewJob {
            id: child,
            job_type: "send_email",
            queue: "mail",
            payload: &payload,
            run_at: Utc::now(),
            recurring_name: None,
            retried_from: Some(id),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        JobsRepo::retried_by(&mut conn, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}")),
        Some(child)
    );
    let row = JobsRepo::get(&mut conn, child)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(row.map(|r| r.retried_from), Some(Some(id)));
    let parent = JobsRepo::get(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        parent.and_then(|r| r.last_error_kind).as_deref(),
        Some("retry")
    );
}

#[sqlx::test]
async fn purge_deletes_old_finished_jobs_and_their_attempts_only(pool: PgPool) {
    let old = seed(&pool, "send_email", -1).await;
    let live = seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    succeed(&mut conn, old).await;
    sqlx::query("UPDATE jobs SET finished_at = now() - interval '30 days' WHERE id = $1")
        .bind(old)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let n = JobsRepo::purge_finished(
        &mut conn,
        Utc::now() - Duration::days(7),
        Utc::now() - Duration::days(30),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(n, 1);
    assert!(
        JobsRepo::get(&mut conn, live)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .is_some()
    );
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = $1")
        .bind(old)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(attempts, 0);
}

#[sqlx::test]
async fn a_late_finish_from_a_stale_try_changes_nothing(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    for attempt in [1, 2] {
        JobsRepo::begin_attempt(&mut conn, id, attempt, "w", None)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let stale = JobsRepo::finish_attempt(
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
    assert!(!stale);
    assert_eq!(state(&pool, id).await, "running");
    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM job_attempts WHERE job_id = $1 AND attempt = 1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outcome.as_deref(), Some("interrupted"));
}

#[sqlx::test]
async fn a_finish_for_a_job_that_is_no_longer_running_changes_nothing(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    sqlx::query("UPDATE jobs SET state = 'killed' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let applied = JobsRepo::finish_attempt(
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
    assert!(!applied);
    assert_eq!(state(&pool, id).await, "killed");
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_attempts WHERE job_id = $1 AND finished_at IS NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(open, 1);
}
