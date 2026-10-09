//! Recurring maintenance jobs: `job_history_purge` closes stranded ledger rows, then deletes
//! finished apalis rows and old recurring-run bookkeeping past their retention window; `data_retention` runs the
//! cross-crate purge queries `postit-data` owns.

use chrono::Utc;
use postit_config::JobsSettings;
use postit_data::job_outbox::JobOutboxRepo;
use postit_data::jobs::JobsRepo;
use postit_data::recurring_runs::RecurringRunsRepo;
use postit_data::retention;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::registry::RegisteredTypes;
use crate::{Job, JobContext, JobError, JobRegistry, JobsError, Queue, RetryPolicy};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct JobHistoryPurge {}

impl Job for JobHistoryPurge {
    const JOB_TYPE: &'static str = "job_history_purge";
    const QUEUE: Queue = Queue::Maintenance;
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataRetention {}

impl Job for DataRetention {
    const JOB_TYPE: &'static str = "data_retention";
    const QUEUE: Queue = Queue::Maintenance;
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
            async move { run_job_history_purge(&pool, &retention, &types).await }
        },
    )
}

/// How long a try may stay open, or a `queued`/`failed` job may stay past due without the
/// storage holding it, before the history purge treats its ledger row as stranded. Well
/// above any handler's runtime.
const STRANDED_AFTER: chrono::TimeDelta = chrono::TimeDelta::hours(24);

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
) -> Result<(u64, u64, u64), JobsError> {
    let cutoff = Utc::now() - STRANDED_AFTER;
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
) -> Result<(), JobError> {
    let (stranded_running, orphaned, unregistered) = sweep_stranded(pool, registered)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let now = Utc::now();
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
    let runs = RecurringRunsRepo::purge_older_than(&mut conn, succeeded_before.min(failed_before))
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    tracing::info!(
        stranded_running,
        orphaned,
        unregistered,
        jobs,
        runs,
        "job history purged"
    );
    Ok(())
}

/// Registers the daily `data_retention` job. `postit-data` owns the purge queries but sits
/// below `postit-jobs`, so `postit-server` calls this function (plan 01, Crate
/// dependencies). Plan 03 phases add their purge queries to [`run_data_retention`].
///
/// # Errors
///
/// Returns [`JobsError`] if the schedule is invalid or the job type is already registered.
pub fn register_data_retention(
    registry: &mut JobRegistry,
    pool: PgPool,
    settings: &JobsSettings,
) -> Result<(), JobsError> {
    registry.register_recurring(
        &settings.schedules.data_retention,
        maintenance_retry(),
        move |_: DataRetention, _: JobContext| {
            let pool = pool.clone();
            async move { run_data_retention(&pool).await }
        },
    )
}

pub(crate) async fn run_data_retention(pool: &PgPool) -> Result<(), JobError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let idempotency_keys = retention::purge_expired_idempotency_keys(&mut conn)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    tracing::info!(idempotency_keys, "data retention applied");
    Ok(())
}
