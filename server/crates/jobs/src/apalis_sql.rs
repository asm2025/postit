//! Raw SQL against the job storage's own tables (`apalis.*`). Runtime `sqlx::query` only:
//! these tables belong to apalis-postgres, not to `postit-data`'s migrations, so the
//! compile-time query cache cannot see them. Every statement here is pinned to the schema
//! recorded in `APALIS_NOTES.md` (item 11).

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
#[cfg(feature = "testkit")]
use ulid::Ulid;
#[cfg(feature = "testkit")]
use uuid::Uuid;

const MAX_DEFER_MILLIS: i64 = 365 * 86_400_000;

/// Moves a running task's next eligible run to `now() + delay`.
///
/// Called by the handler just before it reports a retryable failure: apalis-postgres's ack
/// marks the row `Failed` without touching `run_at`, and its fetch only takes rows with
/// `run_at < now()`, so the retry waits for `delay` in the database rather than in a
/// sleeping handler (`APALIS_NOTES.md` item 8).
pub(crate) async fn defer_next_run(
    pool: &PgPool,
    task_id: &str,
    delay: Duration,
) -> Result<(), sqlx::Error> {
    // Clamped to a year so an absurd policy cannot overflow Postgres's interval type.
    let millis = i64::try_from(delay.as_millis())
        .unwrap_or(i64::MAX)
        .min(MAX_DEFER_MILLIS);
    sqlx::query(
        "UPDATE apalis.jobs SET run_at = now() + ($2::bigint * interval '1 millisecond') \
         WHERE id = $1",
    )
    .bind(task_id)
    .bind(millis)
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether the job storage's schema exists (i.e. `migrate` has run).
pub(crate) async fn storage_ready(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass('apalis.jobs') IS NOT NULL")
        .fetch_one(pool)
        .await
}

/// Deletes postit's finished jobs: rows that succeeded (`Done`) with `done_at` before
/// `succeeded_before`, and rows that were killed or exhausted their retries (`Killed`, or
/// `Failed` with `attempts >= max_attempts`) with `done_at` before `failed_before`. A
/// `Failed` row still under `max_attempts` is a pending retry, never deleted here
/// (`APALIS_NOTES.md` items 8, 10). Returns the number of rows deleted.
pub(crate) async fn purge_finished(
    pool: &PgPool,
    succeeded_before: DateTime<Utc>,
    failed_before: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM apalis.jobs \
         WHERE job_type LIKE 'postit::%' \
           AND ( (status = 'Done' AND done_at < $1) \
              OR (status = 'Killed' AND done_at < $2) \
              OR (status = 'Failed' AND attempts >= max_attempts AND done_at < $2) )",
    )
    .bind(succeeded_before)
    .bind(failed_before)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Task IDs (decoded from the storage's ULID primary key) of every finished job across
/// postit's queues: `Done`, `Killed`, or `Failed` with retries exhausted. Sorted. Exposed
/// through `testkit`, so it is gated on that feature rather than `cfg(test)`.
#[cfg(feature = "testkit")]
pub(crate) async fn finished_task_ids(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    let raw: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM apalis.jobs \
         WHERE job_type LIKE 'postit::%' \
           AND ( status = 'Done' OR status = 'Killed' \
              OR (status = 'Failed' AND attempts >= max_attempts) )",
    )
    .fetch_all(pool)
    .await?;
    let mut ids: Vec<Uuid> = raw
        .iter()
        .filter_map(|id| Ulid::from_string(id).ok())
        .map(Uuid::from)
        .collect();
    ids.sort();
    Ok(ids)
}

/// Moves a finished task's `done_at` further into the past by `by`. Exposed through
/// `testkit`, so it is gated on that feature rather than `cfg(test)`.
#[cfg(feature = "testkit")]
pub(crate) async fn age_finished_job(
    pool: &PgPool,
    task_id: &str,
    by: chrono::Duration,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE apalis.jobs SET done_at = done_at - ($2::bigint * interval '1 millisecond') \
         WHERE id = $1",
    )
    .bind(task_id)
    .bind(by.num_milliseconds())
    .execute(pool)
    .await?;
    Ok(())
}

/// Makes every later push of task `task_id` fail with a unique violation that names no
/// constraint, i.e. an error that must not be mistaken for "already stored". Test-only.
#[cfg(test)]
pub(crate) async fn reject_task_for_test(pool: &PgPool, task_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS apalis.test_rejected_ids (id TEXT PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "CREATE OR REPLACE FUNCTION apalis.test_reject() RETURNS trigger AS $$ \
         BEGIN \
           IF EXISTS (SELECT 1 FROM apalis.test_rejected_ids WHERE id = NEW.id) THEN \
             RAISE EXCEPTION 'rejected by test' USING ERRCODE = 'unique_violation'; \
           END IF; \
           RETURN NEW; \
         END; $$ LANGUAGE plpgsql",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE OR REPLACE TRIGGER test_reject BEFORE INSERT ON apalis.jobs \
         FOR EACH ROW EXECUTE FUNCTION apalis.test_reject()",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO apalis.test_rejected_ids (id) VALUES ($1)")
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// A task's `status`, or `None` if no task has that ID. Test-only.
#[cfg(test)]
pub(crate) async fn task_status(
    pool: &PgPool,
    task_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT status FROM apalis.jobs WHERE id = $1")
        .bind(task_id)
        .fetch_optional(pool)
        .await
}

#[cfg(test)]
mod tests {
    /// Fails when an apalis upgrade renames the table or columns this module's raw SQL
    /// depends on. Fix the SQL (and `APALIS_NOTES.md`), not this test's expectations alone.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn apalis_schema_matches_what_the_raw_sql_expects(pool: sqlx::PgPool) {
        crate::migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("migrate: {e}"));
        for column in [
            "id",
            "job_type",
            "status",
            "attempts",
            "max_attempts",
            "run_at",
            "done_at",
        ] {
            let exists: Option<i32> = sqlx::query_scalar(
                "SELECT 1 FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 AND column_name = $3",
            )
            .bind("apalis")
            .bind("jobs")
            .bind(column)
            .fetch_optional(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("querying information_schema for {column}: {e}"));
            assert!(
                exists.is_some(),
                "apalis.jobs.{column} is missing; update this module's SQL and APALIS_NOTES.md"
            );
        }
    }
}
