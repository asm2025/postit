use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use emixdb::dto::{Pagination, ResultSet};
use postit_core::Clock;
use postit_data::DataError;
use postit_data::jobs::{AttemptRow, Bucket, JobFilter, JobRow, JobState, JobsRepo};
use postit_data::recurring_runs::RecurringRunsRepo;
use sqlx::PgPool;
use uuid::Uuid;

use crate::console_spec::{Actions, ConsoleCatalog, JobSummary};
use crate::error::JobsError;
use crate::queue::JobQueue;

const UNKNOWN_TYPE_REASON: &str = "job type is not registered in this release";

/// Why a console operation failed.
#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    /// No job has that ID.
    #[error("job not found")]
    NotFound,
    /// The action is not allowed for this job; the message says why.
    #[error("action not allowed: {0}")]
    NotAllowed(String),
    /// A database operation failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// Enqueuing the follow-up job failed.
    #[error(transparent)]
    Jobs(#[from] JobsError),
}

impl From<sqlx::Error> for ConsoleError {
    fn from(err: sqlx::Error) -> Self {
        Self::Data(DataError::from(err))
    }
}

/// One job row, safe to serialize: no payload, only the catalog's summary fields.
#[derive(Debug, Clone)]
pub struct JobView {
    /// Job ID.
    pub id: Uuid,
    /// Registered job type name.
    pub job_type: String,
    /// Queue the job runs on.
    pub queue: String,
    /// The stored state, with a queued job that is not yet due shown as `scheduled`.
    pub state: String,
    /// Tries started so far.
    pub attempts: i32,
    /// Retry budget of the job type, when bounded and known.
    pub max_attempts: Option<i32>,
    /// When the job is, or was, due to run.
    pub run_at: DateTime<Utc>,
    /// When the job was enqueued.
    pub created_at: DateTime<Utc>,
    /// Start of the first try.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job reached a state it never leaves.
    pub finished_at: Option<DateTime<Utc>>,
    /// Name of the recurring schedule that created it, if any.
    pub recurring_name: Option<String>,
    /// Allow-listed payload fields; never the raw payload.
    pub summary: JobSummary,
    /// Error kind of the latest try, if it failed.
    pub error_kind: Option<String>,
    /// Error code of the latest try, if it recorded one.
    pub error_code: Option<String>,
    /// Which actions the console offers for this job.
    pub actions: Actions,
    /// Why `actions` is empty, when the job type disables them.
    pub actions_disabled_reason: Option<String>,
}

/// One try of a job as the console shows it.
#[derive(Debug, Clone)]
pub struct AttemptView {
    /// 1-based try number.
    pub attempt: i32,
    /// When the try started.
    pub started_at: DateTime<Utc>,
    /// When the try ended; none while it is running.
    pub finished_at: Option<DateTime<Utc>>,
    /// `host:pid` of the worker that ran the try.
    pub worker: String,
    /// How the try ended.
    pub outcome: Option<String>,
    /// Error kind, if the try failed.
    pub error_kind: Option<String>,
    /// Error code, if the try recorded one.
    pub error_code: Option<String>,
    /// Only what the ledger stored: null unless the job type opted in.
    pub error_message: Option<String>,
}

/// A job with its tries and retry lineage.
#[derive(Debug, Clone)]
pub struct JobDetail {
    /// The job itself.
    pub job: JobView,
    /// Its tries, oldest first.
    pub attempts: Vec<AttemptView>,
    /// The job this one is a manual retry of.
    pub retried_from: Option<Uuid>,
    /// The job that was created to retry this one.
    pub retried_by: Option<Uuid>,
}

/// Dashboard aggregates over the stats window.
#[derive(Debug, Clone)]
pub struct StatsView {
    /// Job counts per state.
    pub states: Vec<(String, i64)>,
    /// Job counts per type.
    pub types: Vec<(String, i64)>,
    /// Due-job counts per queue.
    pub queues: Vec<(String, i64)>,
    /// Attempt outcomes per time bucket.
    pub throughput: Vec<Bucket>,
    /// Length of the window the aggregates cover, in hours.
    pub window_hours: u32,
}

/// A recurring job and its schedule.
#[derive(Debug, Clone)]
pub struct RecurringView {
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

/// The admin job console: reads over the job ledger, shaped for the API.
#[derive(Clone)]
pub struct JobConsole {
    pool: PgPool,
    #[expect(dead_code, reason = "consumed by the console actions (plan task 8)")]
    queue: JobQueue,
    catalog: Arc<ConsoleCatalog>,
    clock: Arc<dyn Clock>,
}

impl JobConsole {
    /// A console over the ledger, enqueuing through `queue` and shaping jobs with `catalog`.
    #[must_use]
    pub fn new(
        pool: PgPool,
        queue: JobQueue,
        catalog: ConsoleCatalog,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            pool,
            queue,
            catalog: Arc::new(catalog),
            clock,
        }
    }

    /// # Errors
    ///
    /// Returns [`ConsoleError`] on a database failure.
    pub async fn list(
        &self,
        filter: JobFilter,
        pagination: Pagination,
    ) -> Result<ResultSet<JobView>, ConsoleError> {
        let mut conn = self.pool.acquire().await?;
        let page = JobsRepo::list(&mut conn, &filter, pagination).await?;
        let now = self.clock.now();
        Ok(ResultSet {
            data: page.data.into_iter().map(|r| self.view(r, now)).collect(),
            total: page.total,
            pagination: page.pagination,
        })
    }

    /// # Errors
    ///
    /// Returns [`ConsoleError::NotFound`] for an unknown id, or a database failure.
    pub async fn detail(&self, id: Uuid) -> Result<JobDetail, ConsoleError> {
        let mut conn = self.pool.acquire().await?;
        let row = JobsRepo::get(&mut conn, id)
            .await?
            .ok_or(ConsoleError::NotFound)?;
        let attempts = JobsRepo::attempts(&mut conn, id).await?;
        let retried_by = JobsRepo::retried_by(&mut conn, id).await?;
        let retried_from = row.retried_from;
        Ok(JobDetail {
            job: self.view(row, self.clock.now()),
            attempts: attempts.into_iter().map(attempt_view).collect(),
            retried_from,
            retried_by,
        })
    }

    /// Aggregates over the last `window_hours` (clamped to 1..=168).
    ///
    /// # Errors
    ///
    /// Returns [`ConsoleError`] on a database failure.
    pub async fn stats(&self, window_hours: u32) -> Result<StatsView, ConsoleError> {
        let hours = window_hours.clamp(1, 168);
        let since = self.clock.now() - Duration::hours(i64::from(hours));
        let bucket_secs = (i64::from(hours) * 3600 / 24).max(300);
        let mut conn = self.pool.acquire().await?;
        let stats = JobsRepo::stats(&mut conn, since, bucket_secs).await?;
        Ok(StatsView {
            states: stats
                .states
                .into_iter()
                .map(|s| (s.state, s.count))
                .collect(),
            types: stats
                .types
                .into_iter()
                .map(|t| (t.job_type, t.count))
                .collect(),
            queues: stats
                .queues
                .into_iter()
                .map(|q| (q.queue, q.count))
                .collect(),
            throughput: stats.throughput,
            window_hours: hours,
        })
    }

    /// # Errors
    ///
    /// Returns [`ConsoleError`] on a database failure.
    pub async fn recurring(&self) -> Result<Vec<RecurringView>, ConsoleError> {
        let mut conn = self.pool.acquire().await?;
        let last = RecurringRunsRepo::last_runs(&mut conn).await?;
        let now = self.clock.now();
        Ok(self
            .catalog
            .recurring()
            .into_iter()
            .map(|(name, schedule)| {
                let run = last.iter().find(|r| r.name == name);
                // The registry rejects bad cron at worker start; the catalog is separate, so log.
                let next_run_at = match Schedule::from_str(&schedule) {
                    Ok(s) => s.after(&now).next(),
                    Err(err) => {
                        tracing::warn!(job = name, %err, "recurring job has an unparsable cron schedule");
                        None
                    }
                };
                RecurringView {
                    name: name.to_owned(),
                    schedule,
                    last_run_at: run.map(|r| r.at),
                    last_outcome: run.and_then(|r| r.outcome.clone()),
                    next_run_at,
                }
            })
            .collect())
    }

    fn view(&self, row: JobRow, now: DateTime<Utc>) -> JobView {
        let spec = self.catalog.get(&row.job_type);
        let (allowed, reason) = match spec {
            Some(spec) => {
                let allowed = spec.allowed();
                let reason = (allowed != Actions::ALL)
                    .then(|| spec.disabled_reason().map(str::to_owned))
                    .flatten();
                (allowed, reason)
            }
            None => (Actions::NONE, Some(UNKNOWN_TYPE_REASON.to_owned())),
        };
        let scheduled = row.state == JobState::Queued && row.run_at > now;
        let state = if scheduled {
            "scheduled"
        } else {
            row.state.as_str()
        };
        let actions = Actions {
            retry: allowed.retry
                && matches!(
                    row.state,
                    JobState::Failed | JobState::Dead | JobState::Killed | JobState::Cancelled
                ),
            // `scheduled` is a derived view of a stored `queued` job, so this covers both.
            cancel: allowed.cancel && row.state == JobState::Queued,
            delete: allowed.delete && row.state.is_finished(),
        };
        JobView {
            id: row.id,
            job_type: row.job_type,
            queue: row.queue,
            state: state.to_owned(),
            attempts: row.attempts,
            max_attempts: row.max_attempts,
            run_at: row.run_at,
            created_at: row.created_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            recurring_name: row.recurring_name,
            summary: spec.map(|s| s.summarize(&row.payload)).unwrap_or_default(),
            error_kind: row.last_error_kind,
            error_code: row.last_error_code,
            actions,
            actions_disabled_reason: reason,
        }
    }
}

fn attempt_view(a: AttemptRow) -> AttemptView {
    AttemptView {
        attempt: a.attempt,
        started_at: a.started_at,
        finished_at: a.finished_at,
        worker: a.worker,
        outcome: a.outcome,
        error_kind: a.error_kind,
        error_code: a.error_code,
        error_message: a.error_message,
    }
}
