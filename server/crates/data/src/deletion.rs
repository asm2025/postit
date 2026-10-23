//! Owner deletion in batches.
//!
//! Deleting a user cascades through every table that references them, which for an owner with
//! years of sends is one statement over millions of rows. The deletion job first empties the
//! biggest tables with [`delete_owned_batch`], a few thousand rows at a time, and then deletes
//! the user, whose cascade is then small.

use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;
use crate::vault::assert_identifier;

/// The owner-scoped tables that grow large enough to delete in batches before the user.
pub const OWNED_BATCH_TABLES: &[&str] = &[
    "direct_sends",
    "email_events",
    "delivery_attempts",
    "inbox_items",
    "post_metric_snapshots",
    "contacts",
];

/// Deletes up to `limit` of `owner`'s rows from `table` and returns how many it deleted. Call it
/// until it returns less than `limit`.
///
/// # Panics
///
/// When `table` is not in [`OWNED_BATCH_TABLES`]. The name is formatted into the statement, so
/// only the listed tables are accepted.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn delete_owned_batch(
    conn: &mut PgConnection,
    owner: UserId,
    table: &str,
    limit: i64,
) -> Result<u64, DataError> {
    assert!(
        OWNED_BATCH_TABLES.contains(&table),
        "{table} is not a batch-deletable table"
    );
    assert_identifier(table);
    let sql = format!(
        "DELETE FROM {table} WHERE id IN (SELECT id FROM {table} WHERE owner_id = $1 LIMIT $2)"
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(owner.as_uuid())
        .bind(limit)
        .execute(&mut *conn)
        .await?
        .rows_affected())
}
