use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    Failed,
}

impl RunOutcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

pub struct RecurringRunsRepo;

impl RecurringRunsRepo {
    /// Returns `true` only for the one caller whose insert created the `(name,
    /// scheduled_for)` row; every other process gets `false` and must not enqueue.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn try_insert_scheduled(
        conn: &mut PgConnection,
        id: Uuid,
        name: &str,
        scheduled_for: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        let inserted = sqlx::query!(
            r#"INSERT INTO job_recurring_runs (id, name, scheduled_for, manual)
               VALUES ($1, $2, $3, FALSE)
               ON CONFLICT (name, scheduled_for) WHERE NOT manual DO NOTHING"#,
            id,
            name,
            scheduled_for,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(inserted == 1)
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn insert_manual(
        conn: &mut PgConnection,
        id: Uuid,
        name: &str,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO job_recurring_runs (id, name, scheduled_for, manual) VALUES ($1, $2, $3, TRUE)",
            id,
            name,
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn set_job_id(
        conn: &mut PgConnection,
        id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE job_recurring_runs SET job_id = $2 WHERE id = $1",
            id,
            job_id
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn finish(
        conn: &mut PgConnection,
        id: Uuid,
        outcome: RunOutcome,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE job_recurring_runs SET outcome = $2, finished_at = $3 WHERE id = $1",
            id,
            outcome.as_str(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn purge_older_than(
        conn: &mut PgConnection,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        let deleted = sqlx::query!(
            "DELETE FROM job_recurring_runs WHERE created_at < $1",
            cutoff
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(deleted)
    }
}
