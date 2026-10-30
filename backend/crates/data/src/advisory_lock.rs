use postit_core::GrantId;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};

use crate::error::DataError;

/// Held while `Db::run_migrations` runs, so concurrent role starts (`api` and `worker` in
/// one deploy) don't race applying migrations.
pub const MIGRATIONS_LOCK_KEY: i64 = 1_000_001;

/// Held by every change to the set of active admins: the bootstrap grant, and the disable,
/// demote and delete paths that check the last-admin guard. Taken before the target row is
/// locked, so two admins acting on each other serialize instead of deadlocking.
pub const ADMIN_SET_LOCK_KEY: i64 = 1_000_002;

/// Held while `postit_jobs::migrate` runs the job storage's own migrations.
pub const JOBS_MIGRATIONS_LOCK_KEY: i64 = 1_000_003;

/// Held while the vault startup guard reads and raises `vault_state`.
pub const VAULT_STATE_LOCK_KEY: i64 = 1_000_004;

/// Held for the whole of a vault re-seal run, so two runs never overlap.
pub const VAULT_ROTATE_LOCK_KEY: i64 = 1_000_005;

/// The first half of a grant's two-int advisory lock key, held while a grant's tokens are
/// refreshed or revoked. Two-int keys live apart from the single bigint keys above.
pub const GRANT_LOCK_NAMESPACE: i32 = 1_007;

/// The first half of a push endpoint's two-int advisory lock key, held while a device is
/// registered, so two users registering the same endpoint serialize instead of both keeping
/// a row or deadlocking on each other's rows. The second half is the first 4 bytes of the
/// endpoint hash, big-endian.
pub const PUSH_ENDPOINT_LOCK_NAMESPACE: i32 = 1_008;

/// The second half of a grant's lock key: the first 4 bytes of the SHA-256 of the grant's
/// UUID, big-endian. Two grants may share a key; they then only wait on each other.
#[must_use]
pub fn key_of(id: GrantId) -> i32 {
    let digest = Sha256::digest(id.as_uuid().as_bytes());
    i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// Takes a transaction-scoped advisory lock on the two-int key `(namespace, key)`, with
/// `pg_advisory_xact_lock(int4, int4)`. Like [`xact_lock`], `conn` must be inside an open
/// transaction.
///
/// # Errors
///
/// Returns [`DataError::Sql`] if the lock statement fails.
pub async fn xact_lock_pair(
    conn: &mut PgConnection,
    namespace: i32,
    key: i32,
) -> Result<(), DataError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(namespace)
        .bind(key)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

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
