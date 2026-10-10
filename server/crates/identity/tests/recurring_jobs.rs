use std::sync::Arc;
use std::time::Duration;

use postit_config::{AuditSettings, JobSchedules};
use postit_core::{IdGenerator, SystemIdGenerator};
use postit_identity::jobs::{IdentityJobs, register};
use postit_jobs::maintenance::{register_data_retention, register_job_history_purge};
use postit_jobs::testkit::{RunningWorker, jobs_settings};
use postit_jobs::{JobQueue, JobRegistry};
use postit_mail::MailLoaders;
use sqlx::PgPool;

const EVERY_SECOND: &str = "* * * * * *";
const RECURRING: [&str; 4] = [
    "audit_retention",
    "data_retention",
    "job_history_purge",
    "purge_pending_users",
];

/// Names among [`RECURRING`] with at least one `succeeded` run row. Sorted.
async fn succeeded_names(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT DISTINCT name FROM job_recurring_runs WHERE outcome = 'succeeded' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("succeeded runs: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn every_real_recurring_job_runs_to_success_on_a_worker(pool: PgPool) {
    let mut settings = jobs_settings();
    settings.schedules = JobSchedules {
        job_history_purge: EVERY_SECOND.into(),
        purge_pending_users: EVERY_SECOND.into(),
        audit_retention: EVERY_SECOND.into(),
        data_retention: EVERY_SECOND.into(),
    };
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    register_job_history_purge(
        &mut registry,
        pool.clone(),
        &settings,
        Arc::new(postit_core::SystemClock),
    )
    .unwrap_or_else(|e| unreachable!("job_history_purge: {e}"));
    register_data_retention(
        &mut registry,
        pool.clone(),
        &settings,
        Arc::new(postit_core::SystemClock),
    )
    .unwrap_or_else(|e| unreachable!("data_retention: {e}"));
    register(
        &mut registry,
        &mut loaders,
        IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(&ids),
            clock: Arc::new(postit_core::SystemClock),
            jobs: JobQueue::new(pool.clone(), ids, Arc::new(postit_core::SystemClock)),
            pending_ttl: Duration::from_hours(30 * 24),
            approval_email_interval: Duration::from_hours(1),
            audit: AuditSettings {
                retention: Duration::from_hours(365 * 24),
                ip_retention: Duration::from_hours(90 * 24),
                pseudonym_key: postit_config::RedactedSecret::from("k".to_string()),
            },
            schedules: settings.schedules.clone(),
        },
    )
    .unwrap_or_else(|e| unreachable!("identity register: {e}"));

    let worker = RunningWorker::start_with(pool.clone(), settings, registry).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut succeeded = succeeded_names(&pool).await;
    while succeeded != RECURRING && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
        succeeded = succeeded_names(&pool).await;
    }
    worker.stop().await;

    assert_eq!(succeeded, RECURRING);
}

#[test]
fn console_describes_the_identity_jobs() {
    let schedules = jobs_settings().schedules;
    let mut catalog = postit_jobs::ConsoleCatalog::default();
    postit_identity::jobs::console(&mut catalog, &schedules);
    assert_eq!(
        catalog.job_types(),
        vec!["audit_retention", "delete_user", "purge_pending_users"]
    );
    assert_eq!(
        catalog.recurring(),
        vec![
            ("audit_retention", schedules.audit_retention.clone()),
            ("purge_pending_users", schedules.purge_pending_users.clone()),
        ]
    );
    for name in catalog.job_types() {
        let spec = catalog.get(name).unwrap_or_else(|| unreachable!("{name}"));
        assert!(!spec.shows_error_message(), "{name} must not show errors");
    }
}
