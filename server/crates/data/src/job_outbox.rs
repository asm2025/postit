use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::jobs::{AttemptOutcome, ErrorKind, LEDGER_SWEEP_WORKER, LEDGER_UNREGISTERED_CODE};

/// The channel `0006_job_outbox.sql`'s trigger notifies on every committed insert.
pub const OUTBOX_CHANNEL: &str = "postit_job_outbox";

#[derive(Debug, Clone)]
pub struct OutboxRow {
    pub id: Uuid,
    pub job_type: String,
    pub payload: Value,
    pub run_at: DateTime<Utc>,
}

/// Only `postit-jobs` uses this: `JobQueue` writes rows, its relay claims and deletes them.
pub struct JobOutboxRepo;

impl JobOutboxRepo {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn insert(
        conn: &mut PgConnection,
        id: Uuid,
        job_type: &str,
        payload: &Value,
        run_at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO job_outbox (id, job_type, payload, run_at) VALUES ($1, $2, $3, $4)",
            id,
            job_type,
            payload,
            run_at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Locks up to `limit` rows whose `job_type` is in `job_types` and whose `id` is not in
    /// `exclude`, oldest first, skipping rows another relay holds. `exclude` lets a relay
    /// pass over rows it already failed to move during the current drain, so rows behind
    /// them are still reached. Rows of other types (a newer release's job waiting for an
    /// upgraded worker) are never claimed, so they can't block a drain loop. Call inside a
    /// transaction; the locks last until it ends.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn claim_batch(
        conn: &mut PgConnection,
        job_types: &[String],
        exclude: &[Uuid],
        limit: i64,
    ) -> Result<Vec<OutboxRow>, DataError> {
        let rows = sqlx::query_as!(
            OutboxRow,
            r#"SELECT id, job_type, payload, run_at FROM job_outbox
               WHERE job_type = ANY($1) AND NOT (id = ANY($2))
               ORDER BY created_at
               FOR UPDATE SKIP LOCKED
               LIMIT $3"#,
            job_types,
            exclude,
            limit,
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows)
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Result<(), DataError> {
        sqlx::query!("DELETE FROM job_outbox WHERE id = $1", id)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// Removes the outbox rows whose `job_type` is not in `registered` and kills their `queued`
    /// ledger jobs that are due before `due_before`, each with a closing attempt of kind
    /// `interrupted` and code [`LEDGER_UNREGISTERED_CODE`] (worker [`LEDGER_SWEEP_WORKER`]);
    /// like [`JobsRepo::kill_orphaned`](crate::jobs::JobsRepo::kill_orphaned), the attempt counts
    /// as a try (`attempts + 1`). One statement: the outbox row is deleted (and re-checked under
    /// its lock, so a relay that claimed it meanwhile wins) before the ledger row is touched.
    /// Returns the number of jobs killed.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn kill_unregistered(
        conn: &mut PgConnection,
        registered: &[String],
        due_before: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        Ok(sqlx::query!(
            "WITH gone AS (
                 DELETE FROM job_outbox o USING jobs j
                 WHERE j.id = o.id AND j.state = 'queued' AND j.run_at < $2
                   AND NOT (o.job_type = ANY($1))
                 RETURNING o.id
             ), killed AS (
                 UPDATE jobs j SET state = 'killed', finished_at = now(), attempts = j.attempts + 1
                 WHERE j.id IN (SELECT id FROM gone) AND j.state = 'queued'
                 RETURNING j.id
             )
             INSERT INTO job_attempts
                 (job_id, attempt, started_at, finished_at, worker, outcome, error_kind, error_code)
             SELECT k.id,
                    COALESCE((SELECT MAX(a.attempt) FROM job_attempts a WHERE a.job_id = k.id), 0) + 1,
                    now(), now(), $3, $4, $5, $6
             FROM killed k",
            registered,
            due_before,
            LEDGER_SWEEP_WORKER,
            AttemptOutcome::Interrupted.as_str(),
            ErrorKind::Interrupted.as_str(),
            LEDGER_UNREGISTERED_CODE,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected())
    }
}
