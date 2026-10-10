use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use emixdb::dto::{Pagination, ResultSet};
use postit_core::Clock;
use postit_data::DataError;
use postit_data::job_outbox::JobOutboxRepo;
use postit_data::jobs::{AttemptRow, Bucket, JobFilter, JobRow, JobState, JobsRepo};
use postit_data::recurring_runs::RecurringRunsRepo;
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

use crate::console_spec::{Actions, ConsoleCatalog, ConsoleSpec, JobSummary};
use crate::error::JobsError;
use crate::job::Queue;
use crate::queue::{EnqueueMeta, JobQueue};
use crate::recurring::RecurringPayload;

const RETRIED_REASON: &str = "job was already retried";
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
        let now = self.clock.now();
        let page = JobsRepo::list(&mut conn, &filter, pagination, now).await?;
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
        let now = self.clock.now();
        let since = now - Duration::hours(i64::from(hours));
        let bucket_secs = (i64::from(hours) * 3600 / 24).max(300);
        let mut conn = self.pool.acquire().await?;
        let stats = JobsRepo::stats(&mut conn, since, now, bucket_secs).await?;
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

    /// Re-runs a finished or failed job as a new job linked to it; returns the new job's ID.
    /// A `failed` job's pending retry is cancelled in the same transaction.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::NotFound`] for an unknown ID, [`ConsoleError::NotAllowed`] when the
    /// type or state forbids it or another action won, or a database failure.
    pub async fn retry(&self, conn: &mut PgConnection, id: Uuid) -> Result<Uuid, ConsoleError> {
        let mut tx = conn.begin().await?;
        let row = JobsRepo::lock_for_action(&mut tx, id)
            .await?
            .ok_or(ConsoleError::NotFound)?;
        self.require(&row, Action::Retry)?;
        if row.state == JobState::Failed {
            if !JobsRepo::cancel(&mut tx, id).await? {
                return Err(ConsoleError::NotAllowed(
                    "job is no longer waiting to retry".into(),
                ));
            }
            JobOutboxRepo::delete(&mut tx, id).await?;
        }
        // A recurring job's payload names its `job_recurring_runs` row, which the original
        // run already closed; the retry gets a fresh manual run instead of reusing it.
        let (run_id, payload) = match row.recurring_name.as_deref() {
            Some(name) => {
                let (run_id, payload) = self.new_manual_run(&mut tx, name).await?;
                (Some(run_id), payload)
            }
            None => (None, row.payload.clone()),
        };
        let meta = EnqueueMeta {
            queue: &row.queue,
            recurring_name: row.recurring_name.as_deref(),
            retried_from: Some(id),
        };
        let new_id = self
            .queue
            .enqueue_raw_in(&mut tx, &row.job_type, &payload, None, &meta)
            .await?;
        if let Some(run_id) = run_id {
            RecurringRunsRepo::set_job_id(&mut tx, run_id, new_id.0).await?;
        }
        tx.commit().await?;
        Ok(new_id.0)
    }

    /// Cancels a job that is still `queued` (or `scheduled`), and drops its outbox row when
    /// the relay has not moved it yet.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::NotFound`] for an unknown ID, [`ConsoleError::NotAllowed`] when the
    /// type or state forbids it or another action won, or a database failure.
    pub async fn cancel(&self, conn: &mut PgConnection, id: Uuid) -> Result<(), ConsoleError> {
        let mut tx = conn.begin().await?;
        let row = JobsRepo::lock_for_action(&mut tx, id)
            .await?
            .ok_or(ConsoleError::NotFound)?;
        self.require(&row, Action::Cancel)?;
        if !JobsRepo::cancel(&mut tx, id).await? {
            return Err(ConsoleError::NotAllowed("job is no longer queued".into()));
        }
        JobOutboxRepo::delete(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes a finished job and its attempts.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::NotFound`] for an unknown ID, [`ConsoleError::NotAllowed`] when the
    /// type or state forbids it or another action won, or a database failure.
    pub async fn delete(&self, conn: &mut PgConnection, id: Uuid) -> Result<(), ConsoleError> {
        let mut tx = conn.begin().await?;
        let row = JobsRepo::lock_for_action(&mut tx, id)
            .await?
            .ok_or(ConsoleError::NotFound)?;
        self.require(&row, Action::Delete)?;
        if !JobsRepo::delete_finished(&mut tx, id).await? {
            return Err(ConsoleError::NotAllowed("job is no longer finished".into()));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Enqueues a manual run of the recurring job `name`; returns the new job's ID.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::NotFound`] when `name` is not a registered recurring job, or a
    /// database failure.
    pub async fn trigger(&self, conn: &mut PgConnection, name: &str) -> Result<Uuid, ConsoleError> {
        let (name, _) = self
            .catalog
            .recurring()
            .into_iter()
            .find(|(n, _)| *n == name)
            .ok_or(ConsoleError::NotFound)?;
        let queue = self
            .catalog
            .get(name)
            .map_or(Queue::Default.as_str(), ConsoleSpec::queue);
        let mut tx = conn.begin().await?;
        let (run_id, payload) = self.new_manual_run(&mut tx, name).await?;
        let meta = EnqueueMeta {
            queue,
            recurring_name: Some(name),
            retried_from: None,
        };
        let new_id = self
            .queue
            .enqueue_raw_in(&mut tx, name, &payload, None, &meta)
            .await?;
        RecurringRunsRepo::set_job_id(&mut tx, run_id, new_id.0).await?;
        tx.commit().await?;
        Ok(new_id.0)
    }

    /// The job type of `id`, for the audit event of an action.
    ///
    /// # Errors
    ///
    /// [`ConsoleError::NotFound`] for an unknown ID, or a database failure.
    pub async fn job_type_of(
        &self,
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<String, ConsoleError> {
        JobsRepo::get(conn, id)
            .await?
            .map(|row| row.job_type)
            .ok_or(ConsoleError::NotFound)
    }

    /// Inserts a manual `job_recurring_runs` row for `name`; returns its ID and the job
    /// payload that points at it.
    async fn new_manual_run(
        &self,
        conn: &mut PgConnection,
        name: &str,
    ) -> Result<(Uuid, serde_json::Value), ConsoleError> {
        let run_id = self.queue.ids().generate();
        RecurringRunsRepo::insert_manual(conn, run_id, name, self.clock.now()).await?;
        let payload = serde_json::to_value(RecurringPayload { run_id }).map_err(JobsError::from)?;
        Ok((run_id, payload))
    }

    /// Refuses with [`ConsoleError::NotAllowed`] unless `action` is offered for `row`.
    fn require(&self, row: &JobRow, action: Action) -> Result<(), ConsoleError> {
        let (actions, reason) =
            allowed_actions(self.catalog.get(&row.job_type), row.state, row.retried);
        let offered = match action {
            Action::Retry => actions.retry,
            Action::Cancel => actions.cancel,
            Action::Delete => actions.delete,
        };
        if offered {
            return Ok(());
        }
        Err(ConsoleError::NotAllowed(reason.unwrap_or_else(|| {
            format!("not available for a {} job", row.state.as_str())
        })))
    }

    fn view(&self, row: JobRow, now: DateTime<Utc>) -> JobView {
        let spec = self.catalog.get(&row.job_type);
        let (actions, reason) = allowed_actions(spec, row.state, row.retried);
        let scheduled = row.state == JobState::Queued && row.run_at > now;
        let state = if scheduled {
            "scheduled"
        } else {
            row.state.as_str()
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

#[derive(Clone, Copy)]
enum Action {
    Retry,
    Cancel,
    Delete,
}

/// Which actions a job in `state` offers, and why they are off when the type forbids them.
/// `spec` is `None` for a type this release does not register.
fn allowed_actions(
    spec: Option<&ConsoleSpec>,
    state: JobState,
    retried: bool,
) -> (Actions, Option<String>) {
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
    let actions = Actions {
        retry: allowed.retry
            && !retried
            && matches!(
                state,
                JobState::Failed | JobState::Dead | JobState::Killed | JobState::Cancelled
            ),
        // `scheduled` is a derived view of a stored `queued` job, so this covers both.
        cancel: allowed.cancel && state == JobState::Queued,
        delete: allowed.delete && state.is_finished(),
    };
    let reason = reason.or_else(|| {
        (retried && allowed.retry && !actions.retry).then(|| RETRIED_REASON.to_owned())
    });
    (actions, reason)
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
