//! Recurring maintenance jobs: `job_history_purge` deletes finished apalis rows and old
//! recurring-run bookkeeping past their retention window; `data_retention` runs the
//! cross-crate purge queries `postit-data` owns.

use chrono::Utc;
use postit_config::JobsSettings;
use postit_data::recurring_runs::RecurringRunsRepo;
use postit_data::retention;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

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
    registry.register_recurring(
        &settings.schedules.job_history_purge,
        maintenance_retry(),
        move |_: JobHistoryPurge, _: JobContext| {
            let pool = pool.clone();
            let retention = retention.clone();
            async move { run_job_history_purge(&pool, &retention).await }
        },
    )
}

pub(crate) async fn run_job_history_purge(
    pool: &PgPool,
    retention: &postit_config::JobHistoryRetention,
) -> Result<(), JobError> {
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
    tracing::info!(jobs, runs, "job history purged");
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
