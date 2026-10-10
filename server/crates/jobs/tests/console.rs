use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_core::SystemIdGenerator;
use postit_data::jobs::JobFilter;
use postit_jobs::{ConsoleCatalog, ConsoleSpec, Job, JobConsole, JobQueue, Queue};
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
            .enum_field("kind", "mail_kind"),
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
    catalog.add::<Mail>(ConsoleSpec::new().disabled("the delivery decides"));
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
        detail.job.actions_disabled_reason.as_deref(),
        Some("the delivery decides")
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
        job.actions_disabled_reason.as_deref(),
        Some("job type is not registered in this release")
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
