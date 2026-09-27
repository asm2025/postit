//! Raw SQL against the job storage's own tables (`apalis.*`). Runtime `sqlx::query` only:
//! these tables belong to apalis-postgres, not to `postit-data`'s migrations, so the
//! compile-time query cache cannot see them. Every statement here is pinned to the schema
//! recorded in `APALIS_NOTES.md` (item 11).

use std::time::Duration;

use sqlx::PgPool;

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
