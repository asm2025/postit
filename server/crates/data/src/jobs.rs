//! The job ledger: one `jobs` row per job and one `job_attempts` row per try. This is the
//! write side used by the worker wrapper and the admin console.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::users::ParseEnumError;

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($(#[$vmeta])* $variant),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            /// The stable name stored in the database.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ParseEnumError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|v| v.as_str() == value)
                    .ok_or_else(|| ParseEnumError(value.to_string()))
            }
        }
    };
}

str_enum!(
    /// Lifecycle state of a ledger job.
    JobState {
    /// Waiting for a worker, or scheduled for later.
    Queued => "queued",
    /// A worker is executing a try.
    Running => "running",
    /// Finished successfully.
    Succeeded => "succeeded",
    /// The last try failed and a retry is waiting.
    Failed => "failed",
    /// Every retry failed.
    Dead => "dead",
    /// Aborted without retry: a fatal error, a panic, or an unregistered job type.
    Killed => "killed",
    /// Cancelled by an admin before it finished.
    Cancelled => "cancelled",
});

impl JobState {
    /// A state a job never leaves on its own.
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Dead | Self::Killed | Self::Cancelled
        )
    }
}

str_enum!(
    /// How one try of a job ended.
    AttemptOutcome {
    /// The handler returned success.
    Succeeded => "succeeded",
    /// The handler failed and a retry was scheduled.
    Retrying => "retrying",
    /// The handler failed and no retries remain.
    Failed => "failed",
    /// The handler was aborted without retry.
    Killed => "killed",
    /// The worker was lost, or the job was closed by the history purge.
    Interrupted => "interrupted",
    /// An admin cancelled the job.
    Cancelled => "cancelled",
});

str_enum!(
    /// Why a try failed.
    ErrorKind {
    /// The handler asked for a retry.
    Retry => "retry",
    /// The retry budget was used up.
    RetriesExhausted => "retries_exhausted",
    /// The handler failed permanently.
    Fatal => "fatal",
    /// No worker registers the job type.
    UnknownType => "unknown_type",
    /// The handler panicked.
    Panic => "panic",
    /// The worker was lost mid-try.
    Interrupted => "interrupted",
});

impl ErrorKind {
    #[must_use]
    /// The error code recorded when the failure has no handler-supplied one.
    pub fn default_code(self) -> Option<&'static str> {
        match self {
            Self::RetriesExhausted => Some("max_attempts"),
            Self::UnknownType => Some("not_registered"),
            Self::Panic => Some("handler_panic"),
            Self::Interrupted => Some("worker_lost"),
            Self::Retry | Self::Fatal => None,
        }
    }
}

/// Error code of the attempt the history purge adds when it closes a stranded `queued` or
/// `failed` job (kind `interrupted`): the job storage no longer holds the job, so no try will
/// ever record it. Typically its try ran while the ledger could not be written.
pub const LEDGER_ORPHAN_CODE: &str = "ledger_orphan";

/// Error code of the attempt the history purge adds when it kills a job whose type no worker
/// registers and whose outbox row has waited past the stranded threshold (kind `interrupted`).
/// The threshold outlasts any rolling deploy, so a newer release's type is not killed by an
/// older worker in practice. Plan 03 re-queues unknown types at dispatch (P5 deferred item 2).
pub const LEDGER_UNREGISTERED_CODE: &str = "type_unregistered";

/// Worker name of that attempt: it records the sweep, not a try of the job.
pub const LEDGER_SWEEP_WORKER: &str = "ledger_sweep";

/// A ledger row to create at enqueue time.
pub struct NewJob<'a> {
    /// ID of the job; also its storage task ID.
    pub id: Uuid,
    /// Registered job type name.
    pub job_type: &'a str,
    /// Queue the job runs on.
    pub queue: &'a str,
    /// Serialised job payload.
    pub payload: &'a Value,
    /// Earliest time the job may run.
    pub run_at: DateTime<Utc>,
    /// Name of the recurring schedule that created it, if any.
    pub recurring_name: Option<&'a str>,
    /// The job this one is a manual retry of, if any.
    pub retried_from: Option<Uuid>,
}

/// What a worker should do after looking up a job's ledger row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// Run the handler.
    Run,
    /// An admin cancelled the job; finish it without running the handler.
    Cancelled,
    /// No ledger row: the job was cancelled, then deleted by an admin or purged, while its
    /// storage task was still pending. Finish it without running the handler.
    Missing,
    /// The ledger row is already finished (`succeeded`, `dead`, `killed`): the storage
    /// redelivered a task whose ack was lost. Finish it without running the handler again.
    Finished,
}

/// The failure recorded on a closed attempt.
pub struct AttemptError<'a> {
    /// Kind of error that ended the attempt.
    pub kind: ErrorKind,
    /// Stable machine-readable code, when the error has one.
    pub code: Option<&'a str>,
    /// Pass `Some` only for job types that opted in to error messages.
    pub message: Option<&'a str>,
}

/// How an attempt ended and where the job goes next.
pub struct Finish<'a> {
    /// How the attempt ended.
    pub outcome: AttemptOutcome,
    /// The state the job moves to.
    pub state: JobState,
    /// Set for `Failed` (a retry is waiting): when it becomes due.
    pub next_run_at: Option<DateTime<Utc>>,
    /// The failure, for attempts that did not succeed.
    pub error: Option<AttemptError<'a>>,
}

/// Reads and writes of the job ledger (`jobs`, `job_attempts`).
pub struct JobsRepo;

impl JobsRepo {
    /// Inserts a `queued` job.
    ///
    /// # Errors
    /// Returns [`DataError`] if the insert fails.
    pub async fn insert(conn: &mut PgConnection, job: &NewJob<'_>) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO jobs (id, job_type, queue, payload, run_at, recurring_name, retried_from)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            job.id,
            job.job_type,
            job.queue,
            job.payload,
            job.run_at,
            job.recurring_name,
            job.retried_from,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Opens attempt `attempt`, closing any attempt left open by a lost worker as
    /// `interrupted`. Starts its own transaction on `conn` (a savepoint if `conn` is
    /// already inside one).
    ///
    /// # Errors
    /// Returns [`DataError`] if a statement fails.
    pub async fn begin_attempt(
        conn: &mut PgConnection,
        id: Uuid,
        attempt: i32,
        worker: &str,
        max_attempts: Option<i32>,
    ) -> Result<Begin, DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        let state: Option<String> =
            sqlx::query_scalar!("SELECT state FROM jobs WHERE id = $1 FOR UPDATE", id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(state) = state else {
            tx.rollback().await?;
            return Ok(Begin::Missing);
        };
        match state.as_str() {
            "cancelled" => {
                tx.commit().await?;
                return Ok(Begin::Cancelled);
            }
            "succeeded" | "dead" | "killed" => {
                tx.commit().await?;
                return Ok(Begin::Finished);
            }
            _ => {}
        }
        sqlx::query!(
            "UPDATE job_attempts SET finished_at = now(), outcome = 'interrupted',
                    error_kind = 'interrupted', error_code = 'worker_lost'
             WHERE job_id = $1 AND finished_at IS NULL AND attempt <> $2",
            id,
            attempt,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO job_attempts (job_id, attempt, worker) VALUES ($1, $2, $3)
             ON CONFLICT (job_id, attempt)
             DO UPDATE SET started_at = now(), finished_at = NULL, outcome = NULL,
                           error_kind = NULL, error_code = NULL, error_message = NULL, worker = $3",
            id,
            attempt,
            worker,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE jobs SET state = 'running', attempts = GREATEST(attempts, $2),
                    max_attempts = COALESCE($3, max_attempts), started_at = now()
             WHERE id = $1",
            id,
            attempt,
            max_attempts,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Begin::Run)
    }

    /// Closes attempt `attempt` and moves the job to `finish.state`. Returns `false`, changing
    /// nothing, when the attempt is no longer open (a newer try closed it as `interrupted`) or
    /// the job is no longer `running`: a late finish from a stale try must not overwrite the
    /// state the newer try or an admin set.
    ///
    /// # Errors
    /// Returns [`DataError`] if a statement fails.
    pub async fn finish_attempt(
        conn: &mut PgConnection,
        id: Uuid,
        attempt: i32,
        finish: &Finish<'_>,
    ) -> Result<bool, DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        // Lock order: the `jobs` row first, as `begin_attempt` does, so a stale try
        // finishing while a newer one begins cannot deadlock on the two tables.
        sqlx::query_scalar!(
            "SELECT 1 AS \"one!\" FROM jobs WHERE id = $1 FOR UPDATE",
            id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (kind, code, message) = match &finish.error {
            Some(e) => (Some(e.kind.as_str()), e.code, e.message),
            None => (None, None, None),
        };
        let closed = sqlx::query!(
            "UPDATE job_attempts SET finished_at = now(), outcome = $3,
                    error_kind = $4, error_code = $5, error_message = $6
             WHERE job_id = $1 AND attempt = $2 AND finished_at IS NULL",
            id,
            attempt,
            finish.outcome.as_str(),
            kind,
            code,
            message,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if closed == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        // `finished_at` only for a state the job never leaves: a `failed` job is waiting
        // for its retry, so it keeps `finished_at` null (stats, purge and the list's time
        // filter all treat `finished_at` as "terminal since").
        let moved = sqlx::query!(
            "UPDATE jobs SET state = $2,
                    finished_at = CASE WHEN $4 THEN now() END,
                    run_at = COALESCE($3, run_at)
             WHERE id = $1 AND state = 'running'",
            id,
            finish.state.as_str(),
            finish.next_run_at,
            finish.state.is_finished(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if moved == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    /// `queued` or `failed` (a retry is waiting) -> `cancelled`. Returns whether this call
    /// changed the row; `false` means the job is running, finished, or gone. The console
    /// offers cancel only for `queued`; it calls this for `failed` only to retire the pending
    /// retry of a job it re-runs as a new job.
    ///
    /// # Errors
    /// Returns [`DataError`] if the update fails.
    pub async fn cancel(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError> {
        let n = sqlx::query!(
            "UPDATE jobs SET state = 'cancelled', finished_at = now()
             WHERE id = $1 AND state IN ('queued', 'failed')",
            id,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    /// Kills `running` jobs whose open attempt started before `started_before`, closing that
    /// attempt as `interrupted` / `worker_lost`, as a new try does for a lost worker. For the
    /// history purge: a try whose end was never recorded leaves its job `running` with no
    /// later try to close it. Returns the number of jobs killed.
    ///
    /// # Errors
    /// Returns [`DataError`] if a statement fails.
    pub async fn kill_stranded_running(
        conn: &mut PgConnection,
        started_before: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        // Lock order: the `jobs` rows first, as `begin_attempt` and `finish_attempt` do.
        let ids: Vec<Uuid> = sqlx::query_scalar!(
            "SELECT j.id FROM jobs j
             WHERE j.state = 'running'
               AND EXISTS (SELECT 1 FROM job_attempts a
                           WHERE a.job_id = j.id AND a.finished_at IS NULL AND a.started_at < $1)
             ORDER BY j.id
             FOR UPDATE OF j",
            started_before,
        )
        .fetch_all(&mut *tx)
        .await?;
        if ids.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        // Only jobs whose old attempt this statement closed are killed: a job that a try
        // finished or began before the lock was taken is left alone.
        let killed = sqlx::query!(
            "WITH closed AS (
                 UPDATE job_attempts SET finished_at = now(), outcome = $3,
                        error_kind = $4, error_code = $5
                 WHERE job_id = ANY($1) AND finished_at IS NULL AND started_at < $2
                 RETURNING job_id
             )
             UPDATE jobs SET state = 'killed', finished_at = now()
             WHERE id IN (SELECT job_id FROM closed) AND state = 'running'",
            &ids,
            started_before,
            AttemptOutcome::Interrupted.as_str(),
            ErrorKind::Interrupted.as_str(),
            ErrorKind::Interrupted.default_code(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(killed)
    }

    /// `queued` and `failed` jobs due before `due_before` that have no outbox row, i.e. that
    /// the relay already moved into the job storage. Sorted. The history purge asks the
    /// storage which of them it still holds and passes the rest to [`Self::kill_orphaned`].
    ///
    /// # Errors
    /// Returns [`DataError`] if the query fails.
    pub async fn stale_waiting(
        conn: &mut PgConnection,
        due_before: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, DataError> {
        Ok(sqlx::query_scalar!(
            "SELECT j.id FROM jobs j
             WHERE j.state IN ('queued', 'failed') AND j.run_at < $1
               AND NOT EXISTS (SELECT 1 FROM job_outbox o WHERE o.id = j.id)
             ORDER BY j.id",
            due_before,
        )
        .fetch_all(&mut *conn)
        .await?)
    }

    /// Kills the jobs in `ids` that are still `queued` or `failed`, due before `due_before`
    /// and without an outbox row, each with a closing attempt of kind `interrupted` and code
    /// [`LEDGER_ORPHAN_CODE`] (worker [`LEDGER_SWEEP_WORKER`]). The attempt counts as a try (`jobs.attempts + 1`), so `attempts`
    /// equals the number of attempt rows: the due try may have run unrecorded, and this row is
    /// its terminal record. A console retry creates a new job (`retried_from`), so the bump never
    /// exhausts a retry. Returns the number of jobs killed.
    ///
    /// # Errors
    /// Returns [`DataError`] if the statement fails.
    pub async fn kill_orphaned(
        conn: &mut PgConnection,
        ids: &[Uuid],
        due_before: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        // One statement: the `jobs` rows are locked (and their state re-checked) before the
        // attempts are written, so a try beginning meanwhile either wins or is not swept.
        Ok(sqlx::query!(
            "WITH killed AS (
                 UPDATE jobs j SET state = 'killed', finished_at = now(), attempts = j.attempts + 1
                 WHERE j.id = ANY($1) AND j.state IN ('queued', 'failed') AND j.run_at < $2
                   AND NOT EXISTS (SELECT 1 FROM job_outbox o WHERE o.id = j.id)
                 RETURNING j.id
             )
             INSERT INTO job_attempts
                 (job_id, attempt, started_at, finished_at, worker, outcome, error_kind, error_code)
             SELECT k.id,
                    COALESCE((SELECT MAX(a.attempt) FROM job_attempts a WHERE a.job_id = k.id), 0) + 1,
                    now(), now(), $3, $4, $5, $6
             FROM killed k",
            ids,
            due_before,
            LEDGER_SWEEP_WORKER,
            AttemptOutcome::Interrupted.as_str(),
            ErrorKind::Interrupted.as_str(),
            LEDGER_ORPHAN_CODE,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected())
    }

    /// Deletes a finished job (`succeeded`, `dead`, `killed`, `cancelled`) and, by cascade,
    /// its attempts. Returns whether a row was deleted.
    ///
    /// # Errors
    /// Returns [`DataError`] if the delete fails.
    pub async fn delete_finished(conn: &mut PgConnection, id: Uuid) -> Result<bool, DataError> {
        let n = sqlx::query!(
            "DELETE FROM jobs WHERE id = $1 AND state IN ('succeeded', 'dead', 'killed', 'cancelled')",
            id,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(n == 1)
    }
}

/// A ledger job as shown in the console list and detail views.
#[derive(Debug, Clone)]
pub struct JobRow {
    /// ID of the job.
    pub id: Uuid,
    /// Registered job type name.
    pub job_type: String,
    /// Queue the job runs on.
    pub queue: String,
    /// Serialised job payload.
    pub payload: Value,
    /// Current lifecycle state.
    pub state: JobState,
    /// When the job is (or was) due to run; moves forward when a retry is scheduled.
    pub run_at: DateTime<Utc>,
    /// Tries started so far.
    pub attempts: i32,
    /// Retry budget of the job type; `None` until the first try reports it, or unbounded.
    pub max_attempts: Option<i32>,
    /// Set for jobs created by a recurring schedule.
    pub recurring_name: Option<String>,
    /// The job this one is a manual retry of.
    pub retried_from: Option<Uuid>,
    /// When the ledger row was created.
    pub created_at: DateTime<Utc>,
    /// Start of the first try.
    pub started_at: Option<DateTime<Utc>>,
    /// Set once the job reaches a state it never leaves; null while a retry is waiting.
    pub finished_at: Option<DateTime<Utc>>,
    /// Error kind of the most recent attempt, if it recorded one.
    pub last_error_kind: Option<String>,
    /// Error code of the most recent attempt, if it recorded one.
    pub last_error_code: Option<String>,
}

/// One try of a job.
#[derive(Debug, Clone)]
pub struct AttemptRow {
    /// 1-based try number.
    pub attempt: i32,
    /// When the try started.
    pub started_at: DateTime<Utc>,
    /// Null while the try is running.
    pub finished_at: Option<DateTime<Utc>>,
    /// `host:pid` of the worker that ran the try, or [`LEDGER_SWEEP_WORKER`] for the attempt
    /// the history purge adds when it closes an orphaned job.
    pub worker: String,
    /// How the try ended; null while it is running.
    pub outcome: Option<String>,
    /// Error kind, when the try failed.
    pub error_kind: Option<String>,
    /// Error code, when the try recorded one.
    pub error_code: Option<String>,
    /// Stored only for job types that opted in.
    pub error_message: Option<String>,
}

/// List filter. `state` is a string so it can carry the derived `scheduled` state
/// (`queued` with `run_at` in the future); `queued` then means due and waiting.
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    /// State to match; may be the derived `scheduled` state.
    pub state: Option<String>,
    /// Job type to match.
    pub job_type: Option<String>,
    /// Earliest time, inclusive: finish time for finished jobs, due time for the rest.
    pub from: Option<DateTime<Utc>>,
    /// Latest time, inclusive, measured as for `from`.
    pub to: Option<DateTime<Utc>>,
}

/// Jobs in one (derived) state.
#[derive(Debug, Clone)]
pub struct StateCount {
    /// The state name.
    pub state: String,
    /// How many jobs are in it.
    pub count: i64,
}

/// Jobs of one type.
#[derive(Debug, Clone)]
pub struct TypeCount {
    /// The job type name.
    pub job_type: String,
    /// How many jobs have it.
    pub count: i64,
}

/// Due jobs waiting for a worker on one queue.
#[derive(Debug, Clone)]
pub struct QueueDepth {
    /// The queue name.
    pub queue: String,
    /// How many due jobs wait on it.
    pub count: i64,
}

/// Attempt outcomes finished within one time bucket.
#[derive(Debug, Clone)]
pub struct Bucket {
    /// Start of the bucket.
    pub start: DateTime<Utc>,
    /// Attempts that succeeded in the bucket.
    pub succeeded: i64,
    /// Attempts that failed in the bucket.
    pub failed: i64,
}

/// Aggregates for the console dashboard.
#[derive(Debug, Clone)]
pub struct JobStatsRow {
    /// Job counts per state.
    pub states: Vec<StateCount>,
    /// Job counts per type.
    pub types: Vec<TypeCount>,
    /// Due-job depth per queue.
    pub queues: Vec<QueueDepth>,
    /// Attempt outcomes over time.
    pub throughput: Vec<Bucket>,
}

struct RawJob {
    id: Uuid,
    job_type: String,
    queue: String,
    payload: Value,
    state: String,
    run_at: DateTime<Utc>,
    attempts: i32,
    max_attempts: Option<i32>,
    recurring_name: Option<String>,
    retried_from: Option<Uuid>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    last_error_kind: Option<String>,
    last_error_code: Option<String>,
}

impl TryFrom<RawJob> for JobRow {
    type Error = DataError;

    fn try_from(r: RawJob) -> Result<Self, DataError> {
        // The `CHECK` constraint makes an unknown state unreachable.
        let state = r
            .state
            .parse::<JobState>()
            .map_err(|e| DataError::Conflict(format!("unknown job state {}", e.0)))?;
        Ok(Self {
            id: r.id,
            job_type: r.job_type,
            queue: r.queue,
            payload: r.payload,
            state,
            run_at: r.run_at,
            attempts: r.attempts,
            max_attempts: r.max_attempts,
            recurring_name: r.recurring_name,
            retried_from: r.retried_from,
            created_at: r.created_at,
            started_at: r.started_at,
            finished_at: r.finished_at,
            last_error_kind: r.last_error_kind,
            last_error_code: r.last_error_code,
        })
    }
}

impl JobsRepo {
    /// One job with the error kind and code of its latest attempt, or `None` if it does not exist.
    ///
    /// # Errors
    /// Returns [`DataError`] if the query fails.
    pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<JobRow>, DataError> {
        let raw = sqlx::query_as!(
            RawJob,
            r#"SELECT j.id, j.job_type, j.queue, j.payload, j.state, j.run_at, j.attempts,
                      j.max_attempts, j.recurring_name, j.retried_from, j.created_at,
                      j.started_at, j.finished_at,
                      (SELECT a.error_kind FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_kind,
                      (SELECT a.error_code FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_code
               FROM jobs j WHERE j.id = $1"#,
            id,
        )
        .fetch_optional(&mut *conn)
        .await?;
        raw.map(JobRow::try_from).transpose()
    }

    /// Like [`Self::get`], but locks the row (`FOR UPDATE`) until the caller's transaction
    /// ends, so concurrent console actions on one job serialize.
    ///
    /// # Errors
    /// Returns [`DataError`] if the query fails.
    pub async fn lock_for_action(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<JobRow>, DataError> {
        let raw = sqlx::query_as!(
            RawJob,
            r#"SELECT j.id, j.job_type, j.queue, j.payload, j.state, j.run_at, j.attempts,
                      j.max_attempts, j.recurring_name, j.retried_from, j.created_at,
                      j.started_at, j.finished_at,
                      (SELECT a.error_kind FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_kind,
                      (SELECT a.error_code FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_code
               FROM jobs j WHERE j.id = $1 FOR UPDATE OF j"#,
            id,
        )
        .fetch_optional(&mut *conn)
        .await?;
        raw.map(JobRow::try_from).transpose()
    }

    /// Attempts of a job, ordered by attempt number.
    ///
    /// # Errors
    /// Returns [`DataError`] if the query fails.
    pub async fn attempts(conn: &mut PgConnection, id: Uuid) -> Result<Vec<AttemptRow>, DataError> {
        Ok(sqlx::query_as!(
            AttemptRow,
            "SELECT attempt, started_at, finished_at, worker, outcome, error_kind, error_code, error_message
             FROM job_attempts WHERE job_id = $1 ORDER BY attempt",
            id,
        )
        .fetch_all(&mut *conn)
        .await?)
    }

    /// The newest job whose `retried_from` is `id`.
    ///
    /// # Errors
    /// Returns [`DataError`] if the query fails.
    pub async fn retried_by(conn: &mut PgConnection, id: Uuid) -> Result<Option<Uuid>, DataError> {
        Ok(sqlx::query_scalar!(
            "SELECT id FROM jobs WHERE retried_from = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
            id,
        )
        .fetch_optional(&mut *conn)
        .await?)
    }

    /// Lists jobs, newest first. The page query and the count query repeat the same `WHERE`
    /// clause: keep them in sync.
    ///
    /// # Errors
    /// Returns [`DataError`] if a query fails.
    pub async fn list(
        conn: &mut PgConnection,
        filter: &JobFilter,
        pagination: Pagination,
    ) -> Result<ResultSet<JobRow>, DataError> {
        let limit = i64::try_from(pagination.page_size).unwrap_or(10);
        let offset = i64::try_from(
            pagination
                .page
                .saturating_sub(1)
                .saturating_mul(pagination.page_size),
        )
        .unwrap_or(0);

        let raw = sqlx::query_as!(
            RawJob,
            r#"SELECT j.id, j.job_type, j.queue, j.payload, j.state, j.run_at, j.attempts,
                      j.max_attempts, j.recurring_name, j.retried_from, j.created_at,
                      j.started_at, j.finished_at,
                      (SELECT a.error_kind FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_kind,
                      (SELECT a.error_code FROM job_attempts a WHERE a.job_id = j.id ORDER BY a.attempt DESC LIMIT 1) AS last_error_code
               FROM jobs j
               WHERE ($1::text IS NULL OR CASE
                          WHEN $1 = 'scheduled' THEN j.state = 'queued' AND j.run_at > now()
                          WHEN $1 = 'queued'    THEN j.state = 'queued' AND j.run_at <= now()
                          ELSE j.state = $1 END)
                 AND ($2::text IS NULL OR j.job_type = $2)
                 AND ($3::timestamptz IS NULL OR
                      CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
                           THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END >= $3)
                 AND ($4::timestamptz IS NULL OR
                      CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
                           THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END <= $4)
               ORDER BY j.created_at DESC, j.id DESC
               LIMIT $5 OFFSET $6"#,
            filter.state,
            filter.job_type,
            filter.from,
            filter.to,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total: Option<i64> = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM jobs j
               WHERE ($1::text IS NULL OR CASE
                          WHEN $1 = 'scheduled' THEN j.state = 'queued' AND j.run_at > now()
                          WHEN $1 = 'queued'    THEN j.state = 'queued' AND j.run_at <= now()
                          ELSE j.state = $1 END)
                 AND ($2::text IS NULL OR j.job_type = $2)
                 AND ($3::timestamptz IS NULL OR
                      CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
                           THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END >= $3)
                 AND ($4::timestamptz IS NULL OR
                      CASE WHEN j.state IN ('succeeded','dead','killed','cancelled')
                           THEN COALESCE(j.finished_at, j.created_at) ELSE j.run_at END <= $4)"#,
            filter.state,
            filter.job_type,
            filter.from,
            filter.to,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(ResultSet {
            data: raw
                .into_iter()
                .map(JobRow::try_from)
                .collect::<Result<_, _>>()?,
            total: u64::try_from(total.unwrap_or(0)).unwrap_or(0),
            pagination: Some(pagination),
        })
    }

    /// Dashboard aggregates; throughput covers attempts finished since `since`, in buckets
    /// of `bucket_secs` seconds.
    ///
    /// # Errors
    /// Returns [`DataError`] if a query fails.
    pub async fn stats(
        conn: &mut PgConnection,
        since: DateTime<Utc>,
        bucket_secs: i64,
    ) -> Result<JobStatsRow, DataError> {
        let states = sqlx::query_as!(
            StateCount,
            r#"SELECT CASE WHEN state = 'queued' AND run_at > now() THEN 'scheduled' ELSE state END AS "state!",
                      COUNT(*) AS "count!"
               FROM jobs GROUP BY 1 ORDER BY 1"#,
        )
        .fetch_all(&mut *conn)
        .await?;
        let types = sqlx::query_as!(
            TypeCount,
            r#"SELECT job_type, COUNT(*) AS "count!" FROM jobs GROUP BY job_type ORDER BY job_type"#,
        )
        .fetch_all(&mut *conn)
        .await?;
        let queues = sqlx::query_as!(
            QueueDepth,
            r#"SELECT queue, COUNT(*) AS "count!" FROM jobs
               WHERE state = 'queued' AND run_at <= now() GROUP BY queue ORDER BY queue"#,
        )
        .fetch_all(&mut *conn)
        .await?;
        let throughput = sqlx::query_as!(
            Bucket,
            r#"SELECT date_bin(make_interval(secs => $2::bigint::double precision), finished_at, TIMESTAMPTZ 'epoch') AS "start!",
                      COUNT(*) FILTER (WHERE outcome = 'succeeded') AS "succeeded!",
                      COUNT(*) FILTER (WHERE outcome IN ('retrying','failed','killed')) AS "failed!"
               FROM job_attempts WHERE finished_at >= $1 GROUP BY 1 ORDER BY 1"#,
            since,
            bucket_secs,
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(JobStatsRow {
            states,
            types,
            queues,
            throughput,
        })
    }

    /// Deletes succeeded jobs finished before `succeeded_before` and dead/killed/cancelled
    /// jobs finished before `failed_before` (attempts cascade). A `failed` job waiting for
    /// its retry is never purged. Returns the number of jobs deleted.
    ///
    /// # Errors
    /// Returns [`DataError`] if the delete fails.
    pub async fn purge_finished(
        conn: &mut PgConnection,
        succeeded_before: DateTime<Utc>,
        failed_before: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        Ok(sqlx::query!(
            "DELETE FROM jobs
             WHERE (state = 'succeeded' AND finished_at < $1)
                OR (state IN ('dead', 'killed', 'cancelled') AND finished_at < $2)",
            succeeded_before,
            failed_before,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected())
    }
}
