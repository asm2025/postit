use sqlx::{PgConnection, PgPool};

use crate::error::DataError;

/// Held while `Db::run_migrations` runs, so concurrent role starts (`api` and `worker` in
/// one deploy) don't race applying migrations.
pub const MIGRATIONS_LOCK_KEY: i64 = 1_000_001;

/// Held while `postit-identity` checks and grants the bootstrap admin, so two concurrent
/// first sign-ins that both match the bootstrap rule can't both become admin.
pub const BOOTSTRAP_ADMIN_LOCK_KEY: i64 = 1_000_002;

/// Takes a transaction-scoped advisory lock: released automatically when `conn`'s
/// transaction ends (commit or rollback). `conn` must be inside an open transaction — a
/// lock taken outside one releases at the end of the single implicit statement, which is
/// never what a caller of this function wants.
///
/// # Errors
///
/// Returns [`DataError::Sql`] if the lock statement fails.
pub async fn xact_lock(conn: &mut PgConnection, key: i64) -> Result<(), DataError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Takes a session-scoped advisory lock on a dedicated connection from `pool`, runs `f`,
/// then releases the lock whether `f` succeeded or not. Used for migrations, which manage
/// their own connections and transactions internally and so can't be wrapped in
/// [`xact_lock`].
///
/// # Errors
///
/// Returns [`DataError::Sql`] if acquiring the connection or either lock statement fails,
/// or `f`'s own error.
pub async fn with_session_lock<F, Fut, T>(pool: &PgPool, key: i64, f: F) -> Result<T, DataError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, DataError>>,
{
    let mut lock_conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *lock_conn)
        .await?;

    let result = f().await;

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(key)
        .execute(&mut *lock_conn)
        .await?;

    result
}
