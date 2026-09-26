use chrono::{Duration, Utc};
use sqlx::PgConnection;

use crate::error::DataError;

/// Clears `ip` on events older than `ip_retention`, then deletes events older than
/// `retention`. Returns `(ip_cleared, deleted)`. The cutoffs are computed here in Rust
/// (`Utc::now() - retention`) and bound as plain `timestamptz` values, rather than binding
/// a `chrono::Duration` for Postgres to subtract as an `interval` — `sqlx`'s Postgres
/// driver has no `Encode` for `chrono::Duration` to `interval`, only its own `PgInterval`
/// type, and a bound `timestamptz` is simpler than introducing that type for one query.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_audit_events(
    conn: &mut PgConnection,
    ip_retention: Duration,
    retention: Duration,
) -> Result<(u64, u64), DataError> {
    let ip_cutoff = Utc::now() - ip_retention;
    let retention_cutoff = Utc::now() - retention;

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

/// Deletes `idempotency_keys` rows past their `expires_at`.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_expired_idempotency_keys(conn: &mut PgConnection) -> Result<u64, DataError> {
    let now = Utc::now();
    let deleted = sqlx::query!("DELETE FROM idempotency_keys WHERE expires_at < $1", now)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(deleted)
}
