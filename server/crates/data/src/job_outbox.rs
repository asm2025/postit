use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

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
}
