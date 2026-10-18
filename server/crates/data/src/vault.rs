//! The vault startup guard and the catalog of sealed columns.

use std::collections::{BTreeMap, BTreeSet};

use postit_vault::Vault;
use sqlx::PgPool;

use crate::advisory_lock::{VAULT_STATE_LOCK_KEY, xact_lock};
use crate::error::DataError;

/// A `bytea` column that holds vault-sealed values.
#[derive(Debug, Clone, Copy)]
pub struct SealedColumn {
    /// Table name.
    pub table: &'static str,
    /// Column holding the sealed bytes.
    pub column: &'static str,
    /// Column holding the row ID the value is sealed for.
    pub id_column: &'static str,
}

/// Every sealed column in the schema. Each table that stores a sealed value adds itself here.
pub const SEALED_COLUMNS: &[SealedColumn] = &[];

/// Table and column names are formatted into SQL, so they must be plain identifiers.
fn assert_identifier(name: &str) {
    assert!(
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
        "sealed column catalog entry is not a plain identifier"
    );
}

/// Which key versions the sealed values in `columns` use, keyed `table.column`. Columns with
/// no sealed values are left out.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn key_versions_in_use(
    pool: &PgPool,
    columns: &[SealedColumn],
) -> Result<BTreeMap<String, BTreeSet<u32>>, DataError> {
    let mut conn = pool.acquire().await?;
    versions_in_use(&mut conn, columns).await
}

async fn versions_in_use(
    conn: &mut sqlx::PgConnection,
    columns: &[SealedColumn],
) -> Result<BTreeMap<String, BTreeSet<u32>>, DataError> {
    let mut out = BTreeMap::new();
    for c in columns {
        assert_identifier(c.table);
        assert_identifier(c.column);
        assert_identifier(c.id_column);
        // Identifiers are asserted plain above and come from the static catalog only.
        let sql = format!(
            "SELECT DISTINCT (get_byte({col}, 1)::bigint << 24) \
             | (get_byte({col}, 2)::bigint << 16) \
             | (get_byte({col}, 3)::bigint << 8) \
             | get_byte({col}, 4)::bigint \
             FROM {table} WHERE {col} IS NOT NULL",
            col = c.column,
            table = c.table,
        );
        let versions: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *conn)
            .await?;
        let set: BTreeSet<u32> = versions
            .into_iter()
            .filter_map(|v| u32::try_from(v).ok())
            .collect();
        if !set.is_empty() {
            out.insert(format!("{}.{}", c.table, c.column), set);
        }
    }
    Ok(out)
}

/// Checks the configured vault against the database before the process serves anything.
///
/// Raises `vault_state` when the vault's newest version is higher, and refuses to start when
/// it is lower ([`DataError::VaultKeyBehind`]) or when a sealed value uses a version the vault
/// lacks ([`DataError::VaultKeyMissing`]). Runs in one transaction under an advisory lock, so
/// concurrent starts agree.
///
/// # Errors
///
/// The errors above, or [`DataError::Sql`] on a database failure.
pub async fn vault_check_startup(
    pool: &PgPool,
    vault: &Vault,
    columns: &[SealedColumn],
) -> Result<(), DataError> {
    let mut tx = pool.begin().await?;
    xact_lock(&mut tx, VAULT_STATE_LOCK_KEY).await?;

    let configured = vault.newest_version();
    let stored: Option<i32> = sqlx::query_scalar("SELECT newest_key_version FROM vault_state")
        .fetch_optional(&mut *tx)
        .await?;
    let stored = stored.and_then(|v| u32::try_from(v).ok());
    match stored {
        Some(stored) if configured < stored => {
            return Err(DataError::VaultKeyBehind { configured, stored });
        }
        Some(stored) if configured == stored => {}
        _ => {
            let value = i32::try_from(configured)
                .map_err(|_| DataError::Conflict("vault key version is too large".into()))?;
            sqlx::query(
                "INSERT INTO vault_state (id, newest_key_version) VALUES (TRUE, $1) \
                 ON CONFLICT (id) DO UPDATE SET newest_key_version = EXCLUDED.newest_key_version, \
                 updated_at = now()",
            )
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
    }

    let held: BTreeSet<u32> = vault.versions().into_iter().collect();
    let in_use = versions_in_use(&mut tx, columns).await?;
    if let Some(version) = in_use
        .values()
        .flatten()
        .copied()
        .find(|v| !held.contains(v))
    {
        return Err(DataError::VaultKeyMissing { version });
    }
    tx.commit().await?;
    Ok(())
}
