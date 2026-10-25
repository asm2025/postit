//! The resumable re-seal routine: moves every sealed value to the vault's newest key version.
//!
//! It rewrites only the wrapped data key, never the ciphertext, and writes each row with a
//! compare-and-swap on the old bytes, so a value an application write replaced meanwhile is
//! left alone. A run can be stopped and started again at any point.

use postit_vault::{SealContext, Sealed, Vault};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::advisory_lock::VAULT_ROTATE_LOCK_KEY;
use crate::error::DataError;
use crate::vault::{SealedColumn, assert_identifier};

/// What one re-seal run did, per column. Counts only.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ResealReport {
    /// One entry per sealed column, in catalog order.
    pub columns: Vec<ColumnReport>,
}

/// Counts for one sealed column.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnReport {
    /// `table.column`.
    pub column: String,
    /// Non-null values read.
    pub scanned: u64,
    /// Values moved to the newest key version.
    pub resealed: u64,
    /// Values left as they were: already newest, changed or deleted meanwhile.
    pub skipped: u64,
}

/// Counts for one batch.
#[derive(Debug, Clone, Copy, Default)]
pub struct BatchOutcome {
    /// Values in the batch.
    pub scanned: u64,
    /// Values rewritten.
    pub resealed: u64,
    /// Values not rewritten.
    pub skipped: u64,
}

/// Re-seals every value in `columns` to the vault's newest key version, `batch_size` rows at a
/// time, under the rotation lock.
///
/// # Errors
///
/// [`DataError::RotationInProgress`] when another run holds the lock,
/// [`DataError::VaultKeyBehind`] when `vault_state` records a newer version than the vault
/// holds, [`DataError::Vault`] when a value cannot be unwrapped, or [`DataError::Sql`].
pub async fn reseal(
    pool: &PgPool,
    vault: &Vault,
    columns: &[SealedColumn],
    batch_size: u32,
) -> Result<ResealReport, DataError> {
    let mut lock_conn = pool.acquire().await?;
    let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(VAULT_ROTATE_LOCK_KEY)
        .fetch_one(&mut *lock_conn)
        .await?;
    if !got {
        return Err(DataError::RotationInProgress);
    }

    let result = run(pool, &mut lock_conn, vault, columns, batch_size).await;

    let unlocked = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(VAULT_ROTATE_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await;
    if unlocked.is_err() {
        // The session lock dies with the connection, so never hand it back to the pool.
        lock_conn.close_on_drop();
    }
    result
}

async fn run(
    pool: &PgPool,
    conn: &mut PgConnection,
    vault: &Vault,
    columns: &[SealedColumn],
    batch_size: u32,
) -> Result<ResealReport, DataError> {
    let configured = vault.newest_version();
    let stored: Option<i32> = sqlx::query_scalar("SELECT newest_key_version FROM vault_state")
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(stored) = stored.and_then(|v| u32::try_from(v).ok())
        && configured < stored
    {
        return Err(DataError::VaultKeyBehind { configured, stored });
    }

    let mut report = ResealReport::default();
    for column in columns {
        let mut done = ColumnReport {
            column: format!("{}.{}", column.table, column.column),
            scanned: 0,
            resealed: 0,
            skipped: 0,
        };
        let mut cursor = None;
        loop {
            let rows = read_batch(pool, column, cursor, batch_size).await?;
            let Some((last, _)) = rows.last() else {
                break;
            };
            cursor = Some(*last);
            let outcome = write_batch(pool, vault, column, rows).await?;
            done.scanned += outcome.scanned;
            done.resealed += outcome.resealed;
            done.skipped += outcome.skipped;
        }
        tracing::info!(
            column = %done.column,
            scanned = done.scanned,
            resealed = done.resealed,
            skipped = done.skipped,
            "vault re-seal finished a column"
        );
        report.columns.push(done);
    }
    Ok(report)
}

/// Reads the next batch of non-null sealed values after `cursor`, in ID order.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
#[cfg(any(test, feature = "testkit"))]
pub async fn read_batch(
    pool: &PgPool,
    column: &SealedColumn,
    cursor: Option<Uuid>,
    batch_size: u32,
) -> Result<Vec<(Uuid, Vec<u8>)>, DataError> {
    read_batch_impl(pool, column, cursor, batch_size).await
}

#[cfg(not(any(test, feature = "testkit")))]
async fn read_batch(
    pool: &PgPool,
    column: &SealedColumn,
    cursor: Option<Uuid>,
    batch_size: u32,
) -> Result<Vec<(Uuid, Vec<u8>)>, DataError> {
    read_batch_impl(pool, column, cursor, batch_size).await
}

async fn read_batch_impl(
    pool: &PgPool,
    column: &SealedColumn,
    cursor: Option<Uuid>,
    batch_size: u32,
) -> Result<Vec<(Uuid, Vec<u8>)>, DataError> {
    assert_identifier(column.table);
    assert_identifier(column.column);
    assert_identifier(column.id_column);
    // Identifiers are asserted plain above and come from the static catalog only.
    let sql = format!(
        "SELECT {id}, {col} FROM {table} \
         WHERE ($1::uuid IS NULL OR {id} > $1) AND {col} IS NOT NULL \
         ORDER BY {id} LIMIT $2",
        id = column.id_column,
        col = column.column,
        table = column.table,
    );
    let rows = sqlx::query_as::<_, (Uuid, Vec<u8>)>(sqlx::AssertSqlSafe(sql))
        .bind(cursor)
        .bind(i64::from(batch_size.max(1)))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Re-wraps each value in `rows` and writes it back only if the stored bytes are still the
/// ones that were read.
///
/// # Errors
///
/// [`DataError::Vault`] when a value is malformed or cannot be unwrapped, or
/// [`DataError::Sql`] on a database failure.
#[cfg(any(test, feature = "testkit"))]
pub async fn write_batch(
    pool: &PgPool,
    vault: &Vault,
    column: &SealedColumn,
    rows: Vec<(Uuid, Vec<u8>)>,
) -> Result<BatchOutcome, DataError> {
    write_batch_impl(pool, vault, column, rows).await
}

#[cfg(not(any(test, feature = "testkit")))]
async fn write_batch(
    pool: &PgPool,
    vault: &Vault,
    column: &SealedColumn,
    rows: Vec<(Uuid, Vec<u8>)>,
) -> Result<BatchOutcome, DataError> {
    write_batch_impl(pool, vault, column, rows).await
}

async fn write_batch_impl(
    pool: &PgPool,
    vault: &Vault,
    column: &SealedColumn,
    rows: Vec<(Uuid, Vec<u8>)>,
) -> Result<BatchOutcome, DataError> {
    // Identifiers are asserted plain by `read_batch`, and again here for direct callers.
    assert_identifier(column.table);
    assert_identifier(column.column);
    assert_identifier(column.id_column);
    let sql = format!(
        "UPDATE {table} SET {col} = $1 WHERE {id} = $2 AND {col} = $3",
        id = column.id_column,
        col = column.column,
        table = column.table,
    );
    let mut outcome = BatchOutcome::default();
    for (id, old) in rows {
        outcome.scanned += 1;
        // The error text, column and row id are not secret; key material never appears.
        let log_failure = |err: &postit_vault::VaultError| {
            tracing::warn!(
                column = format!("{}.{}", column.table, column.column),
                row = %id,
                error = %err,
                "vault rotation could not rewrap a value"
            );
        };
        let sealed = Sealed::<()>::from_bytes(old.clone()).inspect_err(log_failure)?;
        let ctx = SealContext {
            table: column.table,
            column: column.column,
            row: id,
        };
        let Some(new) = vault.rewrap(&sealed, &ctx).inspect_err(log_failure)? else {
            outcome.skipped += 1;
            continue;
        };
        let affected = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .bind(new.as_bytes())
            .bind(id)
            .bind(&old)
            .execute(pool)
            .await?
            .rows_affected();
        if affected == 1 {
            outcome.resealed += 1;
        } else {
            outcome.skipped += 1;
        }
    }
    Ok(outcome)
}
