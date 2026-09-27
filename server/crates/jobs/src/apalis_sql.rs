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
