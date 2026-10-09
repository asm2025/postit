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
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(
        ConsoleSpec::new()
            .id_field("recipient", "recipient_user_id")
            .enum_field("kind", "mail_kind"),
    );
    (JobConsole::new(pool.clone(), queue.clone(), catalog), queue)
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
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().disabled("the delivery decides"));
    let console = JobConsole::new(pool.clone(), queue.clone(), catalog);
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
    let queue = JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator));
    let mut catalog = ConsoleCatalog::default();
    catalog.add::<Mail>(ConsoleSpec::new().recurring("0 0 3 * * *"));
    let rows = JobConsole::new(pool.clone(), queue, catalog)
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
