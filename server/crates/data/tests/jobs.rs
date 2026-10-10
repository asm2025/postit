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
    assert_eq!(begin, Begin::Run(1));
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
        Utc::now(),
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
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((queued.total, scheduled.total), (1, 1));

    // The split follows the `now` passed in (the console's clock), not the database clock.
    let later = JobsRepo::list(
        &mut conn,
        &JobFilter {
            state: Some("queued".into()),
            ..JobFilter::default()
        },
        page,
        Utc::now() + Duration::days(2),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(later.total, 2);
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
        Utc::now(),
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
    let stats = JobsRepo::stats(
        &mut conn,
        Utc::now() - Duration::hours(24),
        Utc::now(),
        3600,
    )
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

fn page(size: u64) -> Pagination {
    Pagination {
        page: 1,
        page_size: size,
    }
}

async fn force_state(pool: &PgPool, id: Uuid, state: &str, finished_days_ago: Option<i32>) {
    sqlx::query(
        "UPDATE jobs SET state = $2,
                finished_at = CASE WHEN $3::int IS NULL THEN NULL
                                   ELSE now() - make_interval(days => $3::int) END
         WHERE id = $1",
    )
    .bind(id)
    .bind(state)
    .bind(finished_days_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
}

#[sqlx::test]
async fn list_filters_by_exact_state(pool: PgPool) {
    let failed = seed(&pool, "send_email", -1).await;
    seed(&pool, "send_email", -1).await;
    force_state(&pool, failed, "failed", None).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let got = JobsRepo::list(
        &mut conn,
        &JobFilter {
            state: Some("failed".into()),
            ..JobFilter::default()
        },
        page(20),
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(got.data.iter().map(|j| j.id).collect::<Vec<_>>(), [failed]);
    assert_eq!(got.total, 1);
}

#[sqlx::test]
async fn list_filters_by_time_range_on_finish_time_or_run_time(pool: PgPool) {
    let old_done = seed(&pool, "send_email", -1).await;
    let recent_done = seed(&pool, "send_email", -1).await;
    let upcoming = seed(&pool, "send_email", 3600).await;
    force_state(&pool, old_done, "succeeded", Some(10)).await;
    force_state(&pool, recent_done, "succeeded", Some(0)).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let ids = |rows: Vec<postit_data::jobs::JobRow>| {
        let mut v: Vec<Uuid> = rows.into_iter().map(|j| j.id).collect();
        v.sort();
        v
    };
    let since_two_days = JobsRepo::list(
        &mut conn,
        &JobFilter {
            from: Some(Utc::now() - Duration::days(2)),
            ..JobFilter::default()
        },
        page(20),
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let mut want = vec![recent_done, upcoming];
    want.sort();
    assert_eq!(ids(since_two_days.data), want);
    let before_two_days = JobsRepo::list(
        &mut conn,
        &JobFilter {
            to: Some(Utc::now() - Duration::days(2)),
            ..JobFilter::default()
        },
        page(20),
        Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(ids(before_two_days.data), [old_done]);
}

#[sqlx::test]
async fn stats_count_scheduled_separately_from_queued(pool: PgPool) {
    seed(&pool, "send_email", -1).await;
    seed(&pool, "send_email", 3600).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let stats = JobsRepo::stats(&mut conn, Utc::now() - Duration::hours(1), Utc::now(), 60)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let count = |s: &str| {
        stats
            .states
            .iter()
            .find(|c| c.state == s)
            .map_or(0, |c| c.count)
    };
    assert_eq!((count("queued"), count("scheduled")), (1, 1));
    let names: Vec<&str> = stats.states.iter().map(|c| c.state.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "states come back ordered");
}

#[sqlx::test]
async fn purge_removes_old_dead_killed_and_cancelled_and_keeps_the_rest(pool: PgPool) {
    let mut ids = Vec::new();
    for (state, days) in [
        ("dead", Some(40)),
        ("killed", Some(40)),
        ("cancelled", Some(40)),
        ("dead", Some(1)),
        ("succeeded", Some(1)),
        ("failed", None),
    ] {
        let id = seed(&pool, "send_email", -1).await;
        force_state(&pool, id, state, days).await;
        ids.push(id);
    }
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let n = JobsRepo::purge_finished(
        &mut conn,
        Utc::now() - Duration::days(7),
        Utc::now() - Duration::days(30),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(n, 3);
    for id in &ids[3..] {
        assert!(
            JobsRepo::get(&mut conn, *id)
                .await
                .unwrap_or_else(|e| unreachable!("{e}"))
                .is_some()
        );
    }
}

/// `(outcome, error_kind, error_code, worker)` per attempt, oldest first.
async fn attempt_rows(
    pool: &PgPool,
    id: Uuid,
) -> Vec<(Option<String>, Option<String>, Option<String>, String)> {
    sqlx::query_as(
        "SELECT outcome, error_kind, error_code, worker FROM job_attempts
         WHERE job_id = $1 ORDER BY attempt",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"))
}

/// A job whose try 1 began `started_hours_ago` and never finished.
async fn running_since(pool: &PgPool, started_hours_ago: i32) -> Uuid {
    let id = seed(pool, "send_email", 0).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    sqlx::query(
        "UPDATE job_attempts SET started_at = now() - make_interval(hours => $2) WHERE job_id = $1",
    )
    .bind(id)
    .bind(started_hours_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    id
}

/// `jobs.attempts` and the number of `job_attempts` rows of the job.
async fn attempts_and_rows(pool: &PgPool, id: Uuid) -> (i32, i64) {
    sqlx::query_as(
        "SELECT j.attempts, (SELECT count(*) FROM job_attempts a WHERE a.job_id = j.id)
         FROM jobs j WHERE j.id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "shorthand for expected nullable columns"
)]
fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[sqlx::test]
async fn a_stranded_running_job_is_killed_with_its_attempt_interrupted(pool: PgPool) {
    let stranded = running_since(&pool, 30).await;
    let recent = running_since(&pool, 1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let swept = JobsRepo::kill_stranded_running(&mut conn, Utc::now() - Duration::hours(24))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(swept, 1);
    assert_eq!(state(&pool, stranded).await, "killed");
    assert_eq!(
        attempt_rows(&pool, stranded).await,
        vec![(
            s("interrupted"),
            s("interrupted"),
            s("worker_lost"),
            "w1".into()
        )]
    );
    assert_eq!(state(&pool, recent).await, "running");
    assert_eq!(
        attempt_rows(&pool, recent).await,
        vec![(None, None, None, "w1".into())]
    );
    // The sweep closes an existing attempt: no new try, the counts still agree.
    assert_eq!(attempts_and_rows(&pool, stranded).await, (1, 1));
}

#[sqlx::test]
async fn stale_waiting_lists_old_queued_and_failed_jobs_without_an_outbox_row(pool: PgPool) {
    let old_queued = seed(&pool, "send_email", -30 * 3600).await;
    let old_failed = seed(&pool, "send_email", -30 * 3600).await;
    force_state(&pool, old_failed, "failed", None).await;
    let old_running = seed(&pool, "send_email", -30 * 3600).await;
    force_state(&pool, old_running, "running", None).await;
    seed(&pool, "send_email", -3600).await; // recent
    let unrelayed = seed(&pool, "send_email", -30 * 3600).await;
    sqlx::query("INSERT INTO job_outbox (id, job_type, payload, run_at) VALUES ($1, 'send_email', '{}', now())")
        .bind(unrelayed)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let mut want = vec![old_queued, old_failed];
    want.sort();
    assert_eq!(
        JobsRepo::stale_waiting(&mut conn, Utc::now() - Duration::hours(24))
            .await
            .unwrap_or_else(|e| unreachable!("{e}")),
        want
    );
}

#[sqlx::test]
async fn an_orphaned_job_is_killed_with_a_ledger_orphan_attempt(pool: PgPool) {
    let queued = seed(&pool, "send_email", -30 * 3600).await;
    let failed = seed(&pool, "send_email", -30 * 3600).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, failed, 1, "w1", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(
        &mut conn,
        failed,
        1,
        &Finish {
            outcome: AttemptOutcome::Retrying,
            state: JobState::Failed,
            next_run_at: Some(Utc::now() - Duration::hours(30)),
            error: Some(AttemptError {
                kind: ErrorKind::Retry,
                code: None,
                message: None,
            }),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let recent = seed(&pool, "send_email", -3600).await;
    let killed = JobsRepo::kill_orphaned(
        &mut conn,
        &[queued, failed, recent],
        Utc::now() - Duration::hours(24),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(killed, 2);
    let orphan = (
        s("interrupted"),
        s("interrupted"),
        s("ledger_orphan"),
        "ledger_sweep".to_string(),
    );
    assert_eq!(state(&pool, queued).await, "killed");
    assert_eq!(attempt_rows(&pool, queued).await, vec![orphan.clone()]);
    assert_eq!(state(&pool, failed).await, "killed");
    assert_eq!(
        attempt_rows(&pool, failed).await,
        vec![(s("retrying"), s("retry"), None, "w1".into()), orphan]
    );
    assert_eq!(state(&pool, recent).await, "queued");
    assert!(attempt_rows(&pool, recent).await.is_empty());
    // The orphan attempt counts as a try: `attempts` equals the number of attempt rows.
    assert_eq!(attempts_and_rows(&pool, queued).await, (1, 1));
    assert_eq!(attempts_and_rows(&pool, failed).await, (2, 2));
    assert_eq!(attempts_and_rows(&pool, recent).await, (0, 0));
    let finished: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT finished_at FROM jobs WHERE id = $1")
            .bind(queued)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        finished.is_some(),
        "a swept job ages out by the failed retention"
    );
}

#[sqlx::test]
async fn throughput_is_zero_filled_to_one_bucket_per_hour(pool: PgPool) {
    let id = seed(&pool, "send_email", -1).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    succeed(&mut conn, id).await;
    // A try lost with its worker counts as failed.
    let lost = seed(&pool, "send_email", -1).await;
    for worker in ["w1", "w2"] {
        JobsRepo::begin_attempt(&mut conn, lost, 1, worker, None)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let now = Utc::now();
    let stats = JobsRepo::stats(&mut conn, now - Duration::hours(24), now, 3600)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    // Bucket starts are aligned to the hour and both ends are included: the hour holding
    // `now - 24h` through the hour holding `now`, 25 buckets.
    assert_eq!(stats.throughput.len(), 25);
    assert_eq!(stats.throughput.iter().map(|b| b.failed).sum::<i64>(), 1);
    assert!(
        stats
            .throughput
            .windows(2)
            .all(|w| w[1].start - w[0].start == Duration::hours(1))
    );
    assert_eq!(
        stats.throughput.iter().filter(|b| b.succeeded > 0).count(),
        1
    );
    assert_eq!(stats.throughput.iter().map(|b| b.succeeded).sum::<i64>(), 1);
}

#[sqlx::test]
async fn a_redelivered_try_is_recorded_as_a_new_attempt(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::finish_attempt(
        &mut conn,
        id,
        1,
        &Finish {
            outcome: AttemptOutcome::Retrying,
            state: JobState::Failed,
            next_run_at: Some(Utc::now()),
            error: Some(AttemptError {
                kind: ErrorKind::Retry,
                code: Some("smtp"),
                message: None,
            }),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    // The storage lost the ack and hands the same try back under the same number.
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w2", Some(3))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(begin, Begin::Run(2));
    let rows: Vec<(i32, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT attempt, outcome, error_code FROM job_attempts WHERE job_id = $1 ORDER BY attempt",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        rows,
        [
            (1, Some("retrying".into()), Some("smtp".into())),
            (2, None, None),
        ]
    );
}

#[sqlx::test]
async fn a_redelivered_open_try_is_closed_as_interrupted(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::begin_attempt(&mut conn, id, 1, "w1", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    // The worker died mid-try; the storage redelivers the task under the same number.
    let begin = JobsRepo::begin_attempt(&mut conn, id, 1, "w2", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(begin, Begin::Run(2));
    let first: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM job_attempts WHERE job_id = $1 AND attempt = 1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(first.as_deref(), Some("interrupted"));
}

#[sqlx::test]
async fn set_max_attempts_fills_only_an_unset_limit(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    let id = Uuid::now_v7();
    let payload = json!({});
    JobsRepo::insert(&mut conn, &new_job(id, &payload))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::set_max_attempts(&mut conn, id, Some(5))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    JobsRepo::set_max_attempts(&mut conn, id, Some(9))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let max: Option<i32> = sqlx::query_scalar("SELECT max_attempts FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(max, Some(5));
}
