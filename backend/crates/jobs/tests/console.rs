use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_core::SystemIdGenerator;
use postit_data::jobs::{JobFilter, JobsRepo};
use postit_jobs::{ConsoleCatalog, ConsoleSpec, DisabledReason, Job, JobConsole, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Mail {
    recipient: uuid::Uuid,
    kind: String,
    body: String,
}
impl Job for Mail {
    const JOB_TYPE: &'static str = "send_email";
    const QUEUE: Queue = Queue::Mail;
}

fn console(pool: &PgPool) -> (JobConsole, JobQueue) {
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(
        ConsoleSpec::new()
            .id_field("recipient", "recipient_user_id")
            .enum_field(
                "kind",
                "mail_kind",
                &["user_pending_approval", "user_approved"],
            ),
    );
    (
        JobConsole::new(
            pool.clone(),
            queue.clone(),
            catalog,
            Arc::new(postit_core::SystemClock),
        ),
        queue,
    )
}

fn mail(kind: &str, body: &str) -> Mail {
    Mail {
        recipient: uuid::Uuid::now_v7(),
        kind: kind.into(),
        body: body.into(),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_listed_job_shows_a_summary_and_never_the_payload(pool: PgPool) {
    let (console, queue) = console(&pool);
    queue
        .enqueue(&mail("user_approved", "Dear Ada <ada@example.com>"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let page = console
        .list(
            JobFilter::default(),
            Pagination {
                page: 1,
                page_size: 20,
            },
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(page.total, 1);
    let job = &page.data[0];
    assert_eq!(
        job.summary.0.get("mail_kind").map(String::as_str),
        Some("user_approved")
    );
    let rendered = format!("{job:?}");
    assert!(!rendered.contains("ada@example.com") && !rendered.contains("Dear Ada"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn actions_follow_state_and_the_registration(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = queue
        .enqueue(&mail("a", ""))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    let detail = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(detail.job.actions.cancel && !detail.job.actions.retry && !detail.job.actions.delete);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_disabled_type_has_no_actions_and_a_reason(pool: PgPool) {
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().disabled(DisabledReason::PublishingManaged));
    let console = JobConsole::new(
        pool.clone(),
        queue.clone(),
        catalog,
        Arc::new(postit_core::SystemClock),
    );
    let id = queue
        .enqueue(&mail("a", ""))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    let detail = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!detail.job.actions.cancel);
    assert_eq!(
        detail.job.actions_disabled_reason,
        Some(DisabledReason::PublishingManaged)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unknown_id_is_not_found(pool: PgPool) {
    let (console, _) = console(&pool);
    assert!(matches!(
        console.detail(uuid::Uuid::now_v7()).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn recurring_lists_registered_schedules_with_a_next_run(pool: PgPool) {
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().recurring("0 0 3 * * *"));
    let rows = JobConsole::new(
        pool.clone(),
        queue,
        catalog,
        Arc::new(postit_core::SystemClock),
    )
    .recurring()
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(rows.len(), 1);
    assert!(rows[0].next_run_at.is_some());
    assert_eq!(rows[0].schedule, "0 0 3 * * *");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stats_report_a_scheduled_job_separately(pool: PgPool) {
    let (console, queue) = console(&pool);
    queue
        .enqueue_at(
            &mail("a", ""),
            chrono::Utc::now() + chrono::Duration::hours(1),
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stats = console
        .stats(24)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        stats
            .states
            .iter()
            .any(|(s, n)| s == "scheduled" && *n == 1)
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_action_matrix_follows_the_stored_state(pool: PgPool) {
    let (console, queue) = console(&pool);
    // (state, run_at offset in hours, retry, cancel, delete)
    let rows = [
        ("queued", 0, false, true, false),
        ("queued", 1, false, true, false),
        ("running", 0, false, false, false),
        ("succeeded", 0, false, false, true),
        ("failed", 0, true, false, false),
        ("dead", 0, true, false, true),
        ("killed", 0, true, false, true),
        ("cancelled", 0, true, false, true),
    ];
    for (state, hours, retry, cancel, delete) in rows {
        let id = queue
            .enqueue(&mail("a", ""))
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .0;
        sqlx::query(
            "UPDATE jobs SET state = $2, run_at = now() + make_interval(hours => $3) WHERE id = $1",
        )
        .bind(id)
        .bind(state)
        .bind(hours)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
        let job = console
            .detail(id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .job;
        let got = (job.actions.retry, job.actions.cancel, job.actions.delete);
        assert_eq!(got, (retry, cancel, delete), "state {state} +{hours}h");
        assert!(job.actions_disabled_reason.is_none());
        assert_eq!(job.state, if hours > 0 { "scheduled" } else { state });
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unregistered_type_has_every_action_off_and_the_reason(pool: PgPool) {
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let id = queue
        .enqueue(&mail("a", ""))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    let console = JobConsole::new(
        pool.clone(),
        queue,
        ConsoleCatalog::default(),
        Arc::new(postit_core::SystemClock),
    );
    let job = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .job;
    assert_eq!(
        (job.actions.retry, job.actions.cancel, job.actions.delete),
        (false, false, false)
    );
    assert_eq!(
        job.actions_disabled_reason,
        Some(DisabledReason::TypeNotRegistered)
    );
}

/// A queued job shows as `scheduled` until the console's clock reaches its `run_at`, and is
/// `queued` from that exact instant on.
#[sqlx::test(migrations = "../data/migrations")]
async fn a_queued_job_is_scheduled_until_the_clock_reaches_its_run_at(pool: PgPool) {
    use chrono::{Duration, TimeZone, Utc};
    use postit_core::{Clock, TestClock};

    let (_, queue) = console(&pool);
    let run_at = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
    let id = queue
        .enqueue_at(&mail("a", ""), run_at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0;
    let state_at = |now: chrono::DateTime<Utc>| {
        let console = JobConsole::new(
            pool.clone(),
            queue.clone(),
            ConsoleCatalog::default(),
            Arc::new(TestClock::new(now)),
        );
        async move {
            console
                .detail(id)
                .await
                .unwrap_or_else(|e| unreachable!("{e}"))
                .job
                .state
        }
    };

    let clock = TestClock::new(run_at);
    assert_eq!(
        state_at(clock.now() - Duration::seconds(1)).await,
        "scheduled"
    );
    assert_eq!(state_at(clock.now()).await, "queued");
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"))
}

async fn enqueue_mail(queue: &JobQueue) -> uuid::Uuid {
    queue
        .enqueue(&mail("a", ""))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .0
}

async fn set_state(pool: &PgPool, id: uuid::Uuid, state: &str) {
    sqlx::query(
        "UPDATE jobs SET state = $2,
                finished_at = CASE WHEN $2 = 'failed' THEN NULL ELSE now() END
         WHERE id = $1",
    )
    .bind(id)
    .bind(state)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
}

fn recurring_console(pool: &PgPool) -> JobConsole {
    let queue = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().recurring("0 0 3 * * *"));
    JobConsole::new(
        pool.clone(),
        queue,
        catalog,
        Arc::new(postit_core::SystemClock),
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_marks_the_job_and_removes_its_unrelayed_outbox_row(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    console
        .cancel(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(outbox, 0);
    let detail = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.job.state, "cancelled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_is_refused_for_a_running_job(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    JobsRepo::begin_attempt(&mut *conn(&pool).await, id, 1, "w", None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        console.cancel(&mut *conn(&pool).await, id).await,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_is_refused_for_a_failed_job_waiting_to_retry(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "failed").await;
    assert!(matches!(
        console.cancel(&mut *conn(&pool).await, id).await,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn actions_on_an_unknown_job_are_not_found(pool: PgPool) {
    let (console, _) = console(&pool);
    let id = uuid::Uuid::now_v7();
    let mut c = conn(&pool).await;
    assert!(matches!(
        console.retry(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
    assert!(matches!(
        console.cancel(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
    assert!(matches!(
        console.delete(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
    assert!(matches!(
        console.job_type_of(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unregistered_type_refuses_every_action(pool: PgPool) {
    let (_, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "dead").await;
    let console = JobConsole::new(
        pool.clone(),
        queue,
        ConsoleCatalog::default(),
        Arc::new(postit_core::SystemClock),
    );
    let mut c = conn(&pool).await;
    assert!(matches!(
        console.retry(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
    assert!(matches!(
        console.delete(&mut c, id).await,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_creates_a_linked_job_and_leaves_the_original_untouched(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "dead").await;
    let new_id = console
        .retry(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_ne!(new_id, id);
    let detail = console
        .detail(new_id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.retried_from, Some(id));
    let original = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(original.job.state, "dead");
    assert_eq!(original.retried_by, Some(new_id));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retrying_a_failed_job_cancels_its_pending_retry(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "failed").await;
    console
        .retry(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        console
            .detail(id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .job
            .state,
        "cancelled"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_concurrent_retries_of_one_job_make_one_new_job(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "dead").await;
    let (a, b) = tokio::join!(
        async { console.retry(&mut *conn(&pool).await, id).await },
        async { console.retry(&mut *conn(&pool).await, id).await },
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
}

/// Retry and cancel race on a `failed` job: cancel is never offered for it, so whichever
/// order the row lock serializes them in, only the retry succeeds.
#[sqlx::test(migrations = "../data/migrations")]
async fn a_concurrent_retry_and_cancel_of_a_failed_job_cannot_both_win(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "failed").await;
    let (retry, cancel) = tokio::join!(
        async { console.retry(&mut *conn(&pool).await, id).await },
        async { console.cancel(&mut *conn(&pool).await, id).await },
    );
    assert!(retry.is_ok());
    assert!(matches!(
        cancel,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retrying_a_dead_recurring_job_uses_a_new_run(pool: PgPool) {
    let console = recurring_console(&pool);
    let first = console
        .trigger(&mut *conn(&pool).await, "send_email")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    set_state(&pool, first, "dead").await;
    let second = console
        .retry(&mut *conn(&pool).await, first)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let run_of = |id: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            let payload: serde_json::Value =
                sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|e| unreachable!("{e}"));
            payload["run_id"].clone()
        }
    };
    let (a, b) = (run_of(first).await, run_of(second).await);
    assert!(a.is_string() && b.is_string());
    assert_ne!(a, b);
    let runs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM job_recurring_runs WHERE name = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(runs, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_removes_only_finished_jobs(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    assert!(console.delete(&mut *conn(&pool).await, id).await.is_err());
    console
        .cancel(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    console
        .delete(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        console.detail(id).await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn trigger_enqueues_a_manual_run_for_a_recurring_job(pool: PgPool) {
    let console = recurring_console(&pool);
    let id = console
        .trigger(&mut *conn(&pool).await, "send_email")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let detail = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(detail.job.recurring_name.as_deref(), Some("send_email"));
    assert_eq!(
        console
            .job_type_of(&mut *conn(&pool).await, id)
            .await
            .unwrap_or_else(|e| unreachable!("{e}")),
        "send_email"
    );
    assert!(matches!(
        console.trigger(&mut *conn(&pool).await, "nope").await,
        Err(postit_jobs::ConsoleError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_retried_job_no_longer_offers_retry_and_a_second_retry_is_refused(pool: PgPool) {
    let (console, queue) = console(&pool);
    let id = enqueue_mail(&queue).await;
    set_state(&pool, id, "dead").await;
    let before = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(before.job.actions.retry);
    console
        .retry(&mut *conn(&pool).await, id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));

    let after = console
        .detail(id)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!after.job.actions.retry);
    assert!(after.job.actions.delete);
    assert!(after.job.actions_disabled_reason.is_some());
    let listed = console
        .list(
            JobFilter::default(),
            Pagination {
                page: 1,
                page_size: 20,
            },
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let row = listed.data.iter().find(|j| j.id == id);
    assert!(row.is_some_and(|j| !j.actions.retry));
    assert!(matches!(
        console.retry(&mut *conn(&pool).await, id).await,
        Err(postit_jobs::ConsoleError::NotAllowed(_))
    ));
}
