//! Wire types of the admin job console.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_jobs::{AttemptView, JobDetail, JobView, RecurringView, StatsView};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Where a job is in its life. Clients must tolerate values added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStateDto {
    /// Due and waiting for a worker.
    Queued,
    /// Waiting for a future run time.
    Scheduled,
    /// A worker is executing it.
    Running,
    /// Finished successfully.
    Succeeded,
    /// The last attempt failed.
    Failed,
    /// Failed with no attempts left.
    Dead,
    /// Stopped by the worker after a fatal condition.
    Killed,
    /// Cancelled by an admin before it ran.
    Cancelled,
}

impl JobStateDto {
    /// Parses the `snake_case` wire name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }
}

/// The console actions the job currently offers.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct JobActionsDto {
    /// Re-run the job as a new job.
    pub retry: bool,
    /// Cancel the job before it runs.
    pub cancel: bool,
    /// Delete the job and its history.
    pub delete: bool,
}

/// One job.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobDto {
    /// Job ID.
    pub id: Uuid,
    /// Job type name.
    pub job_type: String,
    /// Queue the job runs on.
    pub queue: String,
    /// Current state.
    pub state: JobStateDto,
    /// Attempts made so far.
    pub attempts: i32,
    /// Attempts allowed in total.
    pub max_attempts: Option<i32>,
    /// When the job is (or was) due.
    pub run_at: DateTime<Utc>,
    /// When the job was enqueued.
    pub created_at: DateTime<Utc>,
    /// When the current or last attempt started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job reached a final state.
    pub finished_at: Option<DateTime<Utc>>,
    /// Name of the recurring schedule that enqueued it.
    pub recurring_name: Option<String>,
    /// Non-sensitive fields of the payload.
    pub summary: BTreeMap<String, String>,
    /// Kind of the last error.
    pub error_kind: Option<String>,
    /// Code of the last error.
    pub error_code: Option<String>,
    /// Which actions apply.
    pub actions: JobActionsDto,
    /// Why no action applies, when none does.
    pub actions_disabled_reason: Option<String>,
}

impl From<JobView> for JobDto {
    fn from(v: JobView) -> Self {
        let state = JobStateDto::parse(&v.state).unwrap_or_else(|| {
            tracing::warn!(state = v.state, "job has an unknown state");
            JobStateDto::Failed
        });
        Self {
            id: v.id,
            job_type: v.job_type,
            queue: v.queue,
            state,
            attempts: v.attempts,
            max_attempts: v.max_attempts,
            run_at: v.run_at,
            created_at: v.created_at,
            started_at: v.started_at,
            finished_at: v.finished_at,
            recurring_name: v.recurring_name,
            summary: v.summary.0,
            error_kind: v.error_kind,
            error_code: v.error_code,
            actions: JobActionsDto {
                retry: v.actions.retry,
                cancel: v.actions.cancel,
                delete: v.actions.delete,
            },
            actions_disabled_reason: v.actions_disabled_reason,
        }
    }
}

/// One execution attempt of a job.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AttemptDto {
    /// 1-based attempt number.
    pub attempt: i32,
    /// When the attempt started.
    pub started_at: DateTime<Utc>,
    /// When the attempt ended.
    pub finished_at: Option<DateTime<Utc>>,
    /// The worker that ran it.
    pub worker: String,
    /// How it ended.
    pub outcome: Option<String>,
    /// Kind of the error, if it failed.
    pub error_kind: Option<String>,
    /// Code of the error, if it failed.
    pub error_code: Option<String>,
    /// Error message, if the job type shows it.
    pub error_message: Option<String>,
}

impl From<AttemptView> for AttemptDto {
    fn from(v: AttemptView) -> Self {
        Self {
            attempt: v.attempt,
            started_at: v.started_at,
            finished_at: v.finished_at,
            worker: v.worker,
            outcome: v.outcome,
            error_kind: v.error_kind,
            error_code: v.error_code,
            error_message: v.error_message,
        }
    }
}

/// A job with its attempt history and retry links.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobDetailDto {
    /// The job.
    pub job: JobDto,
    /// Attempts, oldest first.
    pub attempts: Vec<AttemptDto>,
    /// The job this one is a manual retry of.
    pub retried_from: Option<Uuid>,
    /// The job created to retry this one.
    pub retried_by: Option<Uuid>,
}

impl From<JobDetail> for JobDetailDto {
    fn from(v: JobDetail) -> Self {
        Self {
            job: v.job.into(),
            attempts: v.attempts.into_iter().map(Into::into).collect(),
            retried_from: v.retried_from,
            retried_by: v.retried_by,
        }
    }
}

/// A count under a key (state, type or queue).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobCountDto {
    /// The state, type or queue name.
    pub key: String,
    /// How many jobs.
    pub count: u64,
}

/// Attempt outcomes within one time bucket.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ThroughputBucketDto {
    /// Start of the bucket.
    pub start: DateTime<Utc>,
    /// Attempts that succeeded.
    pub succeeded: u64,
    /// Attempts that failed.
    pub failed: u64,
}

/// Dashboard aggregates.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobStatsDto {
    /// Length of the throughput window in hours.
    pub window_hours: u32,
    /// Jobs per state.
    pub states: Vec<JobCountDto>,
    /// Jobs per type.
    pub types: Vec<JobCountDto>,
    /// Due jobs per queue.
    pub queues: Vec<JobCountDto>,
    /// Attempt outcomes over the window.
    pub throughput: Vec<ThroughputBucketDto>,
}

fn counts(rows: Vec<(String, i64)>) -> Vec<JobCountDto> {
    rows.into_iter()
        .map(|(key, count)| JobCountDto {
            key,
            count: count.unsigned_abs(),
        })
        .collect()
}

impl From<StatsView> for JobStatsDto {
    fn from(v: StatsView) -> Self {
        Self {
            window_hours: v.window_hours,
            states: counts(v.states),
            types: counts(v.types),
            queues: counts(v.queues),
            throughput: v
                .throughput
                .into_iter()
                .map(|b| ThroughputBucketDto {
                    start: b.start,
                    succeeded: b.succeeded.unsigned_abs(),
                    failed: b.failed.unsigned_abs(),
                })
                .collect(),
        }
    }
}

/// A recurring job and its schedule.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RecurringJobDto {
    /// Job type name.
    pub name: String,
    /// Cron text of the schedule.
    pub schedule: String,
    /// When the newest run was scheduled.
    pub last_run_at: Option<DateTime<Utc>>,
    /// How the newest run ended.
    pub last_outcome: Option<String>,
    /// When the next run is due.
    pub next_run_at: Option<DateTime<Utc>>,
}

impl From<RecurringView> for RecurringJobDto {
    fn from(v: RecurringView) -> Self {
        Self {
            name: v.name,
            schedule: v.schedule,
            last_run_at: v.last_run_at,
            last_outcome: v.last_outcome,
            next_run_at: v.next_run_at,
        }
    }
}

/// The job a retry or trigger created.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobActionResponse {
    /// The new job's ID.
    pub job_id: Uuid,
}
