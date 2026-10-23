//! Recurring maintenance jobs: `job_history_purge` closes stranded ledger rows, then deletes
//! finished apalis rows and old recurring-run bookkeeping past their retention window; `data_retention` runs the
//! cross-crate purge queries `postit-data` owns.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use postit_config::{JobsSettings, RetentionSettings};
use postit_core::Clock;
use postit_data::job_outbox::JobOutboxRepo;
use postit_data::jobs::JobsRepo;
use postit_data::recurring_runs::RecurringRunsRepo;
use postit_data::retention;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::registry::RegisteredTypes;
use crate::{
    ConsoleCatalog, ConsoleSpec, Job, JobContext, JobError, JobRegistry, JobsError, Queue,
    RetryPolicy,
};

/// Recurring job that purges old job history and closes stranded jobs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct JobHistoryPurge {}

impl Job for JobHistoryPurge {
    const JOB_TYPE: &'static str = "job_history_purge";
    const QUEUE: Queue = Queue::Maintenance;
}

/// Recurring job that applies the data retention windows.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataRetention {}

impl Job for DataRetention {
    const JOB_TYPE: &'static str = "data_retention";
    const QUEUE: Queue = Queue::Maintenance;
}

/// Describes this module's job types to the admin console: `job_history_purge` shows its error
/// messages (they hold no user content), and both recurring jobs report their schedules.
pub fn console(catalog: &mut ConsoleCatalog, settings: &JobsSettings) {
    catalog.add::<JobHistoryPurge>(
        ConsoleSpec::new()
            .show_error_message()
            .recurring(&settings.schedules.job_history_purge),
    );
    catalog.add::<DataRetention>(ConsoleSpec::new().recurring(&settings.schedules.data_retention));
}

fn maintenance_retry() -> RetryPolicy {
    RetryPolicy::Backoff {
        max_attempts: Some(3),
        initial: std::time::Duration::from_secs(60),
        max: std::time::Duration::from_secs(600),
    }
}

/// # Errors
///
/// Returns [`JobsError`] if the schedule is invalid or the job type is already registered.
pub fn register_job_history_purge(
    registry: &mut JobRegistry,
    pool: PgPool,
    settings: &JobsSettings,
    clock: Arc<dyn Clock>,
) -> Result<(), JobsError> {
    let retention = settings.history_retention.clone();
    let types = registry.registered_types();
    registry.register_recurring(
        &settings.schedules.job_history_purge,
        maintenance_retry(),
        move |_: JobHistoryPurge, _: JobContext| {
            let pool = pool.clone();
            let retention = retention.clone();
            let types = types.clone();
            let clock = Arc::clone(&clock);
            async move { run_job_history_purge(&pool, &retention, &types, clock.now()).await }
        },
    )
}

/// How long a try may stay open, or a `queued`/`failed` job may stay past due without the
/// storage holding it, before the history purge treats its ledger row as stranded. Well
/// above any handler's runtime.
const STRANDED_AFTER: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// How long a worker row may go without a heartbeat before the history purge removes it.
const STALE_WORKER_AFTER: chrono::TimeDelta = chrono::TimeDelta::days(1);

/// Closes ledger rows no try will ever close (P8 ruling on stranded rows):
/// - `running` jobs whose open attempt started before the threshold (the end of the try was
///   never recorded) become `killed`, the attempt `interrupted` / `worker_lost`;
/// - `queued`/`failed` jobs due before the threshold, with no outbox row and no task the
///   storage still holds (the try ran while the ledger could not be written), become
///   `killed` with an `interrupted` / `ledger_orphan` attempt.
/// - `queued` jobs due before the threshold whose outbox row is of a type in no worker's
///   registry (a type removed from the release) are deleted from the outbox and become
///   `killed` with an `interrupted` / `type_unregistered` attempt. The threshold outlasts any
///   rolling deploy, so an older worker does not kill a newer release's type in practice;
///   plan 03 re-queues unknown types at dispatch (P5 deferred item 2).
///
/// Returns `(running, orphaned, unregistered)` counts.
async fn sweep_stranded(
    pool: &PgPool,
    registered: &RegisteredTypes,
    now: DateTime<Utc>,
) -> Result<(u64, u64, u64), JobsError> {
    let cutoff = now - STRANDED_AFTER;
    let mut conn = pool.acquire().await?;
    let running = JobsRepo::kill_stranded_running(&mut conn, cutoff).await?;
    let stale = JobsRepo::stale_waiting(&mut conn, cutoff).await?;
    let orphaned = if stale.is_empty() {
        0
    } else {
        let held = crate::backend::held_task_ids(pool, &stale).await?;
        let orphans: Vec<_> = stale.into_iter().filter(|id| !held.contains(id)).collect();
        JobsRepo::kill_orphaned(&mut conn, &orphans, cutoff).await?
    };
    let unregistered =
        JobOutboxRepo::kill_unregistered(&mut conn, &registered.snapshot(), cutoff).await?;
    Ok((running, orphaned, unregistered))
}

/// Sweeps stranded ledger rows first, so they age out with the other finished jobs.
pub(crate) async fn run_job_history_purge(
    pool: &PgPool,
    retention: &postit_config::JobHistoryRetention,
    registered: &RegisteredTypes,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let (stranded_running, orphaned, unregistered) = sweep_stranded(pool, registered, now)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let succeeded_before =
        now - chrono::Duration::from_std(retention.succeeded).unwrap_or_default();
    let failed_before = now - chrono::Duration::from_std(retention.failed).unwrap_or_default();
    let jobs = crate::backend::purge_finished(pool, succeeded_before, failed_before)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let ledger = JobsRepo::purge_finished(&mut conn, succeeded_before, failed_before)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let workers = crate::backend::purge_stale_workers(pool, now - STALE_WORKER_AFTER)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let runs = RecurringRunsRepo::purge_older_than(&mut conn, succeeded_before.min(failed_before))
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    tracing::info!(
        stranded_running,
        orphaned,
        unregistered,
        jobs,
        ledger,
        runs,
        workers,
        "job history purged"
    );
    Ok(())
}

/// Registers the daily `data_retention` job. `postit-data` owns the purge queries but sits
/// below `postit-jobs`, so `postit-server` calls this function (plan 01, Crate
/// dependencies). [`run_data_retention`] applies the `retention` windows to the plan 03 tables.
///
/// # Errors
///
/// Returns [`JobsError`] if the schedule is invalid or the job type is already registered.
pub fn register_data_retention(
    registry: &mut JobRegistry,
    pool: PgPool,
    settings: &JobsSettings,
    retention: RetentionSettings,
    clock: Arc<dyn Clock>,
) -> Result<(), JobsError> {
    registry.register_recurring(
        &settings.schedules.data_retention,
        maintenance_retry(),
        move |_: DataRetention, _: JobContext| {
            let pool = pool.clone();
            let clock = Arc::clone(&clock);
            let retention = retention.clone();
            async move { run_data_retention(&pool, &retention, clock.now()).await }
        },
    )
}

/// The longest retention window the purges honor.
const MAX_RETENTION_WINDOW: std::time::Duration = std::time::Duration::from_hours(100 * 365 * 24);

pub(crate) async fn run_data_retention(
    pool: &PgPool,
    settings: &RetentionSettings,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let retry = |e: postit_data::DataError| JobError::Retry(e.to_string());
    // A window beyond a century cannot be subtracted from a timestamp; clamp it rather than
    // let the conversion fail and turn the cutoff into `now`, which would purge everything.
    let before = |window: std::time::Duration| {
        now - chrono::Duration::from_std(window.min(MAX_RETENTION_WINDOW)).unwrap_or_default()
    };
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let idempotency_keys = retention::purge_expired_idempotency_keys(&mut conn, now)
        .await
        .map_err(retry)?;
    let read_notifications =
        retention::purge_read_notifications(&mut conn, before(settings.notifications_read_after))
            .await
            .map_err(retry)?;
    let delivery_attempts =
        retention::purge_delivery_attempts(&mut conn, before(settings.delivery_attempts_after))
            .await
            .map_err(retry)?;
    // Prompts go first: their window is shorter than the row's, and a prompt must never outlive it.
    let ai_prompts =
        retention::clear_ai_prompts(&mut conn, before(settings.ai_generation_prompt_after))
            .await
            .map_err(retry)?;
    let ai_generations =
        retention::purge_ai_generations(&mut conn, before(settings.ai_generation_row_after))
            .await
            .map_err(retry)?;
    let inbox_items = retention::purge_inbox_items(&mut conn, before(settings.inbox_items))
        .await
        .map_err(retry)?;
    // Events before the sends they belong to, so each count is its own.
    let email_events = retention::purge_email_events(&mut conn, before(settings.email_events))
        .await
        .map_err(retry)?;
    let direct_sends = retention::purge_direct_sends(&mut conn, before(settings.direct_sends))
        .await
        .map_err(retry)?;
    let contact_imports =
        retention::purge_contact_imports(&mut conn, before(settings.contact_imports))
            .await
            .map_err(retry)?;
    let (oauth_states, pending_connections) = retention::purge_expired_connections(&mut conn, now)
        .await
        .map_err(retry)?;
    tracing::info!(
        idempotency_keys,
        read_notifications,
        delivery_attempts,
        ai_prompts,
        ai_generations,
        inbox_items,
        email_events,
        direct_sends,
        contact_imports,
        oauth_states,
        pending_connections,
        "data retention applied"
    );
    Ok(())
}
