//! The job ledger: one `jobs` row per job and one `job_attempts` row per try. This is the
//! write side used by the worker wrapper and the admin console.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::users::ParseEnumError;

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
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

str_enum!(JobState {
    Queued => "queued",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Dead => "dead",
    Killed => "killed",
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

str_enum!(AttemptOutcome {
    Succeeded => "succeeded",
    Retrying => "retrying",
    Failed => "failed",
    Killed => "killed",
    Interrupted => "interrupted",
    Cancelled => "cancelled",
});

str_enum!(ErrorKind {
    Retry => "retry",
    RetriesExhausted => "retries_exhausted",
    Fatal => "fatal",
    UnknownType => "unknown_type",
    Panic => "panic",
    Interrupted => "interrupted",
});

impl ErrorKind {
    #[must_use]
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

pub struct NewJob<'a> {
    pub id: Uuid,
    pub job_type: &'a str,
    pub queue: &'a str,
    pub payload: &'a Value,
    pub run_at: DateTime<Utc>,
    pub recurring_name: Option<&'a str>,
    pub retried_from: Option<Uuid>,
}

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

pub struct AttemptError<'a> {
    pub kind: ErrorKind,
    pub code: Option<&'a str>,
    /// Pass `Some` only for job types that opted in to error messages.
    pub message: Option<&'a str>,
}

pub struct Finish<'a> {
    pub outcome: AttemptOutcome,
    pub state: JobState,
    /// Set for `Failed` (a retry is waiting): when it becomes due.
    pub next_run_at: Option<DateTime<Utc>>,
    pub error: Option<AttemptError<'a>>,
}

pub struct JobsRepo;

impl JobsRepo {
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

    /// # Errors
    /// Returns [`DataError`] if a statement fails.
    pub async fn finish_attempt(
        conn: &mut PgConnection,
        id: Uuid,
        attempt: i32,
        finish: &Finish<'_>,
    ) -> Result<(), DataError> {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        let (kind, code, message) = match &finish.error {
            Some(e) => (Some(e.kind.as_str()), e.code, e.message),
            None => (None, None, None),
        };
        sqlx::query!(
            "UPDATE job_attempts SET finished_at = now(), outcome = $3,
                    error_kind = $4, error_code = $5, error_message = $6
             WHERE job_id = $1 AND attempt = $2",
            id,
            attempt,
            finish.outcome.as_str(),
            kind,
            code,
            message,
        )
        .execute(&mut *tx)
        .await?;
        // `finished_at` only for a state the job never leaves: a `failed` job is waiting
        // for its retry, so it keeps `finished_at` null (stats, purge and the list's time
        // filter all treat `finished_at` as "terminal since").
        sqlx::query!(
            "UPDATE jobs SET state = $2,
                    finished_at = CASE WHEN $4 THEN now() END,
                    run_at = COALESCE($3, run_at)
             WHERE id = $1",
            id,
            finish.state.as_str(),
            finish.next_run_at,
            finish.state.is_finished(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `queued` or `failed` (a retry is waiting) -> `cancelled`. Returns whether this call
    /// changed the row; `false` means the job is running, finished, or gone.
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
