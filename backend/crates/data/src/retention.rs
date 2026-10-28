//! Retention purges: audit events, idempotency keys, and the plan 03 operational data.
//!
//! The plan 03 purges delete in batches of 5000 rows per statement, so a large backlog never
//! holds one long transaction or lock.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgConnection;

use crate::error::DataError;

/// Clears `ip` on events older than `ip_retention`, then deletes events older than
/// `retention`, both measured back from `now`. Returns `(ip_cleared, deleted)`. The cutoffs
/// are computed here in Rust (`now - retention`) and bound as plain `timestamptz` values, rather than binding
/// a `chrono::Duration` for Postgres to subtract as an `interval` — `sqlx`'s Postgres
/// driver has no `Encode` for `chrono::Duration` to `interval`, only its own `PgInterval`
/// type, and a bound `timestamptz` is simpler than introducing that type for one query.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_audit_events(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    ip_retention: Duration,
    retention: Duration,
) -> Result<(u64, u64), DataError> {
    let ip_cutoff = now - ip_retention;
    let retention_cutoff = now - retention;

    let ip_cleared = sqlx::query!(
        "UPDATE audit_events SET ip = NULL WHERE ip IS NOT NULL AND at < $1",
        ip_cutoff,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();

    let deleted = sqlx::query!("DELETE FROM audit_events WHERE at < $1", retention_cutoff)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    Ok((ip_cleared, deleted))
}

/// Deletes `idempotency_keys` rows whose `expires_at` is before `now`.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_expired_idempotency_keys(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
) -> Result<u64, DataError> {
    let deleted = sqlx::query!("DELETE FROM idempotency_keys WHERE expires_at < $1", now)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(deleted)
}

/// Rows per statement in the batched purges.
const BATCH: i64 = 5000;

/// Runs a batched `DELETE` or `UPDATE` until a statement touches fewer rows than the batch.
/// The statement's last parameter is the batch size.
macro_rules! batched {
    ($conn:expr, $sql:literal $(, $bind:expr)*) => {{
        let mut total = 0u64;
        loop {
            let touched = sqlx::query!($sql $(, $bind)*, BATCH)
                .execute(&mut *$conn)
                .await?
                .rows_affected();
            total += touched;
            if i64::try_from(touched).map_or(true, |touched| touched < BATCH) {
                break;
            }
        }
        Ok(total)
    }};
}

/// Deletes notifications that were read before `cutoff`. Unread ones are kept.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_read_notifications(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM notifications WHERE id IN (
             SELECT id FROM notifications WHERE read_at IS NOT NULL AND read_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes delivery attempts that started before `cutoff`.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_delivery_attempts(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM delivery_attempts WHERE id IN (
             SELECT id FROM delivery_attempts WHERE started_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Clears the stored prompt text of AI generations created before `cutoff`. The row stays; see
/// [`purge_ai_generations`].
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn clear_ai_prompts(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "UPDATE ai_generations SET prompt = NULL WHERE id IN (
             SELECT id FROM ai_generations
             WHERE prompt IS NOT NULL AND created_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes AI generations created before `cutoff`.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_ai_generations(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM ai_generations WHERE id IN (
             SELECT id FROM ai_generations WHERE created_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes inbox items stored before `cutoff`.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_inbox_items(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM inbox_items WHERE id IN (
             SELECT id FROM inbox_items WHERE created_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes direct-send rows created before `cutoff`, with their email events. Rows still
/// `queued` belong to a send in progress and are kept.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_direct_sends(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM direct_sends WHERE id IN (
             SELECT id FROM direct_sends
             WHERE state <> 'queued' AND created_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes email events that happened before `cutoff`.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_email_events(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM email_events WHERE id IN (
             SELECT id FROM email_events WHERE at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes finished contact imports, with their reports, created before `cutoff`.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_contact_imports(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64, DataError> {
    batched!(
        conn,
        "DELETE FROM contact_imports WHERE id IN (
             SELECT id FROM contact_imports
             WHERE status <> 'processing' AND created_at < $1 LIMIT $2)",
        cutoff
    )
}

/// Deletes OAuth states and pending connections that expired by `now` (`expires_at <= now`,
/// the boundary every live check shares as `expires_at > now`). Returns
/// `(oauth_states, pending_connections)`. An expired OAuth state that still holds tokens is
/// left for the `oauth_state_purge` job, which revokes them at the provider first.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn purge_expired_connections(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
) -> Result<(u64, u64), DataError> {
    let states: Result<u64, DataError> = batched!(
        conn,
        "DELETE FROM oauth_states WHERE id IN (
             SELECT id FROM oauth_states
             WHERE expires_at <= $1 AND grant_tokens IS NULL LIMIT $2)",
        now
    );
    let pending: Result<u64, DataError> = batched!(
        conn,
        "DELETE FROM pending_connections WHERE id IN (
             SELECT id FROM pending_connections WHERE expires_at <= $1 LIMIT $2)",
        now
    );
    Ok((states?, pending?))
}
