//! Account CSV imports and the rows staged for the import job.
//!
//! An upload stages every row it read, each sealed with its own context, and the job takes
//! them in chunks. Importing accounts is owner-only (plan 01), and so is reading an import, so
//! the scoped methods refuse a delegated scope. The methods taking an `owner` are system calls
//! for the import job.

use chrono::{DateTime, Utc};
use postit_core::{AccountImportId, PlatformId, UserId};
use postit_vault::{SealContext, Sealed};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::text::{corrupt, parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// Where an account import is.
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    ImportState {
        /// Staged rows are being checked, or are about to be.
        Processing => "processing",
        /// A commit run whose file stopped early, waiting for the owner to continue or cancel.
        AwaitingDecision => "awaiting_decision",
        /// Every row was settled.
        Completed => "completed",
        /// The import did not finish.
        Failed => "failed",
        /// The owner canceled it while it waited.
        Canceled => "canceled",
    }
}

text_enum! {
    /// Why reading the file stopped before its end.
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    ImportStop {
        /// The next record would have gone over `accounts.csv_max_rows`.
        MaxRows => "max_rows",
        /// The body went over `accounts.csv_max_bytes`.
        MaxBytes => "max_bytes",
        /// The body could not be read to its end.
        Unreadable => "unreadable",
    }
}

/// An account import.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountImport {
    /// The import.
    pub id: AccountImportId,
    /// The platform the accounts are for.
    pub platform: PlatformId,
    /// Whether accounts are connected; false is the check-only run.
    pub commit: bool,
    /// Where it is.
    pub status: ImportState,
    /// How many rows were staged.
    pub rows_total: i32,
    /// How many rows were settled.
    pub rows_done: i32,
    /// Why reading stopped early, when it did.
    pub stopped: Option<ImportStop>,
    /// The last data row staged.
    pub last_row: i32,
    /// The file line that row starts on.
    pub last_line: i32,
    /// One entry per settled row in file order. Never a cell value.
    pub report: Value,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct ImportRow {
    id: Uuid,
    platform_id: String,
    commit: bool,
    status: String,
    rows_total: i32,
    rows_done: i32,
    stopped: Option<String>,
    last_row: i32,
    last_line: i32,
    report: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ImportRow> for AccountImport {
    type Error = DataError;

    fn try_from(row: ImportRow) -> Result<Self, DataError> {
        Ok(Self {
            id: AccountImportId::from_uuid(row.id),
            platform: PlatformId::new(&row.platform_id).map_err(corrupt)?,
            commit: row.commit,
            status: parse(&row.status)?,
            rows_total: row.rows_total,
            rows_done: row.rows_done,
            stopped: row.stopped.as_deref().map(parse).transpose()?,
            last_row: row.last_row,
            last_line: row.last_line,
            report: row.report,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// What one staged row holds: the cells in header order, or a mark that the record could not be
/// parsed. Sealed, because the cells are credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StagedCells {
    /// A parsed record: names and raw cells in header order, and the optional display name.
    Cells {
        /// Column name and raw cell, in header order.
        cells: Vec<(String, String)>,
        /// The `display_name` column, when the file has one.
        display_name: Option<String>,
    },
    /// A record the CSV reader could not parse.
    Malformed,
}

/// One file row staged for the import job.
#[derive(Debug, Clone)]
pub struct StagedAccountRow {
    /// The row's ID; its `data` is sealed with [`AccountImportRepo::seal_context`] of it.
    pub id: Uuid,
    /// The 1-based data row.
    pub row_no: i32,
    /// The file line the record starts on.
    pub line_no: i32,
    /// The sealed cells.
    pub data: Sealed<StagedCells>,
}

/// Account imports.
pub struct AccountImportRepo;

impl AccountImportRepo {
    /// Where a staged row's cells are sealed: the `data` column of its own row.
    #[must_use]
    pub fn seal_context(row: Uuid) -> SealContext {
        SealContext {
            table: "account_import_rows",
            column: "data",
            row,
        }
    }

    /// Records the start of an import.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
        platform: &PlatformId,
        commit: bool,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query!(
            r#"INSERT INTO account_imports
                   (id, owner_id, platform_id, "commit", created_by, updated_by)
               VALUES ($1, $2, $3, $4, $5, $5)"#,
            id.as_uuid(),
            scope.owner.as_uuid(),
            platform.as_str(),
            commit,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Stages rows for the import job and touches the import, which is its sign of life for the
    /// stale sweep.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::NotFound`] when the
    /// owner has no such import.
    pub async fn stage(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
        rows: &[StagedAccountRow],
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let touched = sqlx::query!(
            "UPDATE account_imports SET updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if touched == 0 {
            return Err(DataError::NotFound);
        }
        for row in rows {
            sqlx::query!(
                "INSERT INTO account_import_rows (id, import_id, owner_id, row_no, line_no, data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
                row.id,
                id.as_uuid(),
                scope.owner.as_uuid(),
                row.row_no,
                row.line_no,
                &row.data as &Sealed<StagedCells>,
            )
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// Records how many rows were staged, where reading stopped, and the state the import
    /// waits in: `Processing` or `AwaitingDecision`.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::NotFound`] when the
    /// owner has no such import.
    #[allow(clippy::too_many_arguments)] // one column per argument, set together
    pub async fn set_total(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
        total: i32,
        stopped: Option<ImportStop>,
        last_row: i32,
        last_line: i32,
        status: ImportState,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let updated = sqlx::query!(
            "UPDATE account_imports
             SET rows_total = $3, stopped = $4, last_row = $5, last_line = $6, status = $7,
                 updated_at = now(), updated_by = $8
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            total,
            stopped.map(ImportStop::as_str),
            last_row,
            last_line,
            status.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Moves a waiting import back to `processing`, keeping its rows. Returns `false` when the
    /// import is in any other state or is not the owner's.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner.
    pub async fn resume(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
    ) -> Result<bool, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let moved = sqlx::query!(
            "UPDATE account_imports
             SET status = 'processing', updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2 AND status = 'awaiting_decision'",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Cancels a waiting import and deletes its staged rows, in one statement. Returns `false`
    /// when the import is in any other state or is not the owner's.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner.
    pub async fn cancel(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
    ) -> Result<bool, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let canceled = sqlx::query_scalar!(
            r#"WITH canceled AS (
                   UPDATE account_imports
                   SET status = 'canceled', updated_at = now(), updated_by = $3
                   WHERE owner_id = $1 AND id = $2 AND status = 'awaiting_decision'
                   RETURNING id),
               gone AS (
                   DELETE FROM account_import_rows
                   WHERE owner_id = $1 AND import_id IN (SELECT id FROM canceled))
               SELECT COUNT(*) AS "canceled!" FROM canceled"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(canceled == 1)
    }

    /// The next `limit` staged rows, lowest row number first. A plain read: nothing is deleted,
    /// so a crash while the rows are checked with the platform loses none. A system call for
    /// the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn next_chunk(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
        limit: i64,
    ) -> Result<Vec<StagedAccountRow>, DataError> {
        sqlx::query!(
            r#"SELECT id, row_no, line_no, data AS "data: Sealed<StagedCells>"
               FROM account_import_rows
               WHERE owner_id = $1 AND import_id = $2
               ORDER BY row_no LIMIT $3"#,
            owner.as_uuid(),
            id.as_uuid(),
            limit,
        )
        .fetch_all(&mut *conn)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| StagedAccountRow {
                    id: row.id,
                    row_no: row.row_no,
                    line_no: row.line_no,
                    data: row.data,
                })
                .collect()
        })
        .map_err(DataError::from)
    }

    /// Settles a chunk, in the caller's transaction: deletes the staged rows up to
    /// `through_row_no`, appends the report `entries` and the newly `seen` remote IDs, and adds
    /// the entries to `rows_done`. A system call for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn settle_chunk(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
        through_row_no: i32,
        entries: &[Value],
        seen: &[String],
    ) -> Result<(), DataError> {
        sqlx::query!(
            "DELETE FROM account_import_rows
             WHERE owner_id = $1 AND import_id = $2 AND row_no <= $3",
            owner.as_uuid(),
            id.as_uuid(),
            through_row_no,
        )
        .execute(&mut *conn)
        .await?;
        let added = Value::Array(entries.to_vec());
        let updated = sqlx::query!(
            "UPDATE account_imports
             SET report = report || $3::jsonb,
                 seen_remote_ids = seen_remote_ids || $4::text[],
                 rows_done = rows_done + $5, updated_at = now()
             WHERE owner_id = $1 AND id = $2",
            owner.as_uuid(),
            id.as_uuid(),
            added,
            seen,
            i32::try_from(entries.len()).unwrap_or(i32::MAX),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// The remote account IDs settled so far. A system call for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn seen(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
    ) -> Result<Vec<String>, DataError> {
        sqlx::query_scalar!(
            "SELECT seen_remote_ids FROM account_imports WHERE owner_id = $1 AND id = $2",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)
    }

    /// Closes a processing import as completed. A system call for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn finish(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE account_imports SET status = 'completed', updated_at = now()
             WHERE owner_id = $1 AND id = $2 AND status = 'processing'",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Marks an unfinished import `failed` and deletes its staged rows, in one statement. An
    /// import already finished is left alone. A system call.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn abandon(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
    ) -> Result<(), DataError> {
        sqlx::query_scalar!(
            r#"WITH failed AS (
                   UPDATE account_imports SET status = 'failed', updated_at = now()
                   WHERE owner_id = $1 AND id = $2
                     AND status IN ('processing', 'awaiting_decision')
                   RETURNING id),
               gone AS (
                   DELETE FROM account_import_rows
                   WHERE owner_id = $1 AND import_id IN (SELECT id FROM failed))
               SELECT COUNT(*) AS "failed!" FROM failed"#,
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(())
    }

    /// One import.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::NotFound`] when the
    /// owner has no such import.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountImportId,
    ) -> Result<AccountImport, DataError> {
        scope.require(Capability::OwnerOnly)?;
        Self::fetch(conn, scope.owner, id).await
    }

    /// One import of `owner`, for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn fetch(
        conn: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
    ) -> Result<AccountImport, DataError> {
        sqlx::query_as!(
            ImportRow,
            r#"SELECT id, platform_id, "commit" AS "commit!", status, rows_total, rows_done,
                      stopped, last_row, last_line, report, created_at, updated_at
               FROM account_imports WHERE owner_id = $1 AND id = $2"#,
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }
}
