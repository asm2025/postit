//! Importing credential accounts from a CSV file.
//!
//! The upload is read as a stream and every record it reads whole is staged, sealed, in
//! chunks; the `account_import` job then checks the staged rows with the platform and, for a
//! commit run, connects them. Each row is connected, reported, and unstaged in one transaction,
//! so a crash or a second run of the job resumes at the first row not yet settled, and every
//! row is reported once and connected at most once. Reports carry row and line numbers and status
//! codes, never a cell value.

use std::collections::{BTreeMap, HashSet};
use std::io;

use csv_async::{AsyncReader, AsyncReaderBuilder, ErrorKind, StringRecord, Trim};
use futures::StreamExt as _;
use postit_core::{
    AccountId, AccountImportId, AgentError, AuditEventId, ConnectMethod, CredentialFields,
    DiscoveredAccount, OptionsSchema, PlatformId, UserId, Violation,
};
use postit_data::account_imports::{
    AccountImport, AccountImportRepo, ImportState, ImportStop, StagedAccountRow, StagedCells,
};
use postit_data::accounts::{AccountRepo, RemoteAccount};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::{Capability, DataError, OwnerScope};
use postit_jobs::{JobContext, JobError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{Connection as _, PgConnection};
use tokio::io::AsyncRead;

use crate::connect::{ConnectInput, redact_violations};
use crate::jobs::AccountImportJob;
use crate::{AccountError, AccountService};

/// The optional column that names the account when a row reaches exactly one.
const DISPLAY_NAME: &str = "display_name";
/// The longest display name an account may be given.
const DISPLAY_NAME_MAX: usize = 100;
/// How many records one staging transaction writes.
const STAGE_CHUNK: usize = 100;

/// How one CSV row ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    /// The credential works and its accounts can be (or were) connected.
    Ok,
    /// The values do not fit the credentials schema.
    InvalidFields,
    /// The platform rejected the credential.
    InvalidCredentials,
    /// Every account the credential reaches is already the owner's.
    AlreadyConnected,
    /// Every account the credential reaches belongs to someone else.
    ConnectedElsewhere,
    /// The credential reaches no account.
    NothingFound,
    /// The platform could not answer.
    PlatformError,
}

/// The report line for one data row. Never carries a field value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRow {
    /// The 1-based data row number.
    pub row: u32,
    /// The file line the record starts on; a quoted cell with line breaks makes it larger than
    /// `row + 1`.
    pub line: u32,
    /// How the row ended.
    pub status: ImportStatus,
    /// The violation codes, for `invalid_fields`.
    pub violations: Vec<String>,
    /// The credential field at fault, when known.
    pub field: Option<String>,
    /// The accounts created or matched, when committed.
    pub account_ids: Vec<AccountId>,
    /// A `display_name` was given but the row reaches more than one account, so it was ignored.
    pub display_name_ignored: bool,
}

/// A row after verification, before it is classified or committed.
struct Checked {
    row: u32,
    line: u32,
    outcome: Result<Verified, ImportRow>,
}

struct Verified {
    fields: CredentialFields,
    discovered: Vec<DiscoveredAccount>,
    display_name: Option<String>,
}

/// A row the CSV reader could not parse (for example the wrong number of columns).
fn malformed_row(row: u32, line: u32) -> ImportRow {
    let mut report = row_report(row, line, ImportStatus::InvalidFields);
    report.violations.push("malformed_row".to_owned());
    report
}

fn row_report(row: u32, line: u32, status: ImportStatus) -> ImportRow {
    ImportRow {
        row,
        line,
        status,
        violations: Vec::new(),
        field: None,
        account_ids: Vec::new(),
        display_name_ignored: false,
    }
}

fn to_u32(n: i32) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

/// The staged form of one parsed record: the non-empty cells in header order, and the display
/// name apart from them.
fn staged_cells(headers: &[String], record: &StringRecord) -> StagedCells {
    let mut cells = Vec::new();
    let mut display_name = None;
    for (name, cell) in headers.iter().zip(record.iter()) {
        let cell = cell.trim();
        if cell.is_empty() {
            continue;
        }
        if name == DISPLAY_NAME {
            display_name = Some(cell.to_owned());
        } else {
            cells.push((name.clone(), cell.to_owned()));
        }
    }
    StagedCells::Cells {
        cells,
        display_name,
    }
}

/// Whether a read error ends the body, and why. A parse error of one record does not.
fn stop_of(error: &csv_async::Error) -> Option<ImportStop> {
    match error.kind() {
        ErrorKind::Io(io) if io.kind() == io::ErrorKind::FileTooLarge => Some(ImportStop::MaxBytes),
        ErrorKind::Io(_) => Some(ImportStop::Unreadable),
        _ => None,
    }
}

/// How an import ended, for its one audit event.
struct Ended<'a> {
    /// `completed` or `canceled`.
    outcome: &'a str,
    /// How many report entries ended each way, by status name.
    counts: BTreeMap<String, u32>,
    /// How many rows the event covers.
    rows: u32,
}

fn job_error(error: impl std::fmt::Display) -> JobError {
    JobError::Retry(error.to_string())
}

/// A file line number as the repository stores it.
fn line_of(position: Option<&csv_async::Position>, previous: i32) -> i32 {
    position.map_or(previous + 1, |p| {
        i32::try_from(p.line()).unwrap_or(i32::MAX)
    })
}

/// How many lines past its first a parsed record runs: one for each line break inside a quoted
/// cell. The cells are not trimmed, so their line breaks are the file's.
fn extra_lines(record: &StringRecord) -> i32 {
    let breaks: usize = record.iter().map(|cell| cell.matches('\n').count()).sum();
    i32::try_from(breaks).unwrap_or(i32::MAX)
}

impl AccountService {
    /// Reads a CSV of credentials and stages every record it reads whole, then queues the
    /// `account_import` job that checks them and, with `commit`, connects the accounts each
    /// valid row reaches.
    ///
    /// The header row lists the credentials schema's field names and may add `display_name`.
    /// A record the CSV reader rejects never stops the others: it is staged as malformed and
    /// reported in its place. Reading stops, keeping every row already read whole, when the
    /// next record would pass `accounts.csv_max_rows`, when the body fails with
    /// [`io::ErrorKind::FileTooLarge`] (`max_bytes`), or when it fails any other way
    /// (`unreadable`). The upload succeeds either way. A dry run that stopped is queued as
    /// usual. A commit run that stopped waits in `awaiting_decision` with nothing connected
    /// until the owner continues or cancels.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, the catalog's errors,
    /// [`AccountError::ConnectMethodUnsupported`] for an OAuth platform, and
    /// [`AccountError::CsvInvalid`] for a bad or unreadable header, before any import exists.
    pub async fn start_import<R>(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        body: R,
        commit: bool,
        // Kept with the import for the audit events its job writes, which have no request.
        audit: AuditContext,
    ) -> Result<AccountImport, AccountError>
    where
        R: AsyncRead + Unpin + Send,
    {
        scope.require(Capability::OwnerOnly)?;
        let resolved = self.resolve(platform).await?;
        let ConnectMethod::Credentials(schema) = &resolved.agent.descriptor().connect else {
            return Err(AccountError::ConnectMethodUnsupported);
        };

        let mut reader = AsyncReaderBuilder::new()
            .trim(Trim::Headers)
            .flexible(false)
            .create_reader(body);
        let headers = read_headers(&mut reader, schema).await?;

        let id = AccountImportId::from_uuid(self.deps.ids.generate());
        let mut conn = self.deps.pool.acquire().await?;
        AccountImportRepo::create(&mut conn, scope, id, platform, commit, audit).await?;
        drop(conn);
        match self
            .stage_import(scope, id, &mut reader, &headers, commit)
            .await
        {
            Ok(import) => Ok(import),
            Err(error) => {
                if let Ok(mut conn) = self.deps.pool.acquire().await {
                    let _ = AccountImportRepo::abandon(&mut conn, scope.owner, id).await;
                }
                Err(error)
            }
        }
    }

    /// Reads the records, stages them a chunk per transaction, and ends with the totals, the
    /// stop, and the queued job in one more.
    async fn stage_import<R>(
        &self,
        scope: &OwnerScope,
        id: AccountImportId,
        reader: &mut AsyncReader<R>,
        headers: &[String],
        commit: bool,
    ) -> Result<AccountImport, AccountError>
    where
        R: AsyncRead + Unpin + Send,
    {
        let max_rows = i32::try_from(self.deps.settings.csv_max_rows).unwrap_or(i32::MAX);
        let mut pending: Vec<StagedAccountRow> = Vec::new();
        let mut record = StringRecord::new();
        let mut rows: i32 = 0;
        // The line the last record read ends on; the header's to begin with.
        let mut last_end: i32 = 1;
        let mut stopped = None;
        loop {
            let (line, end, cells) = match reader.read_record(&mut record).await {
                Ok(false) => break,
                Ok(true) => {
                    let line = line_of(record.position(), last_end);
                    (
                        line,
                        line.saturating_add(extra_lines(&record)),
                        staged_cells(headers, &record),
                    )
                }
                Err(error) => {
                    if let Some(stop) = stop_of(&error) {
                        stopped = Some(stop);
                        break;
                    }
                    // A bad record is reported in its place and the file goes on. It was read
                    // through its line break, so the reader is on the line after it.
                    let line = line_of(error.position(), last_end);
                    let next = i32::try_from(reader.position().line()).unwrap_or(i32::MAX);
                    (line, (next - 1).max(line), StagedCells::Malformed)
                }
            };
            if rows >= max_rows {
                stopped = Some(ImportStop::MaxRows);
                break;
            }
            rows += 1;
            last_end = end;
            let row = self.deps.ids.generate();
            pending.push(StagedAccountRow {
                id: row,
                row_no: rows,
                line_no: line,
                data: self
                    .deps
                    .vault
                    .seal(&cells, &AccountImportRepo::seal_context(row))?,
            });
            if pending.len() >= STAGE_CHUNK {
                let mut tx = self.deps.pool.begin().await?;
                AccountImportRepo::stage(&mut tx, scope, id, &pending).await?;
                tx.commit().await?;
                pending.clear();
            }
        }

        let status = if stopped.is_some() && commit {
            ImportState::AwaitingDecision
        } else {
            ImportState::Processing
        };
        let last_end = if rows == 0 { 0 } else { last_end };
        let mut tx = self.deps.pool.begin().await?;
        AccountImportRepo::stage(&mut tx, scope, id, &pending).await?;
        AccountImportRepo::set_total(&mut tx, scope, id, rows, stopped, rows, last_end, status)
            .await?;
        if status == ImportState::Processing {
            self.queue_import(&mut tx, scope.owner, id).await?;
        }
        let import = AccountImportRepo::get(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(import)
    }

    async fn queue_import(
        &self,
        tx: &mut PgConnection,
        owner: UserId,
        id: AccountImportId,
    ) -> Result<(), AccountError> {
        let job = AccountImportJob {
            import: id.as_uuid(),
            owner: owner.as_uuid(),
        };
        self.deps
            .queue
            .enqueue_in(tx, &job, None)
            .await
            .map_err(|e| AccountError::Internal(e.to_string()))?;
        Ok(())
    }

    /// One import, with its live progress and report. Owner only.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, and [`AccountError::NotFound`]
    /// for an unknown import or another owner's.
    pub async fn get_import(
        &self,
        scope: &OwnerScope,
        id: AccountImportId,
    ) -> Result<AccountImport, AccountError> {
        let mut conn = self.deps.pool.acquire().await?;
        Ok(AccountImportRepo::get(&mut conn, scope, id).await?)
    }

    /// Releases a commit run that stopped early: connects the rows read. Owner only.
    ///
    /// # Errors
    ///
    /// [`AccountError::ImportNotWaiting`] unless the import is waiting for this choice, and
    /// [`AccountError::NotFound`] for an unknown import or another owner's.
    pub async fn continue_import(
        &self,
        scope: &OwnerScope,
        id: AccountImportId,
    ) -> Result<AccountImport, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = self.deps.pool.begin().await?;
        if !AccountImportRepo::resume(&mut tx, scope, id).await? {
            AccountImportRepo::get(&mut tx, scope, id).await?;
            return Err(AccountError::ImportNotWaiting);
        }
        self.queue_import(&mut tx, scope.owner, id).await?;
        let import = AccountImportRepo::get(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(import)
    }

    /// Cancels a commit run that stopped early: nothing is connected and the staged rows are
    /// deleted. Owner only.
    ///
    /// # Errors
    ///
    /// [`AccountError::ImportNotWaiting`] unless the import is waiting for this choice, and
    /// [`AccountError::NotFound`] for an unknown import or another owner's.
    pub async fn cancel_import(
        &self,
        scope: &OwnerScope,
        id: AccountImportId,
        audit: AuditContext,
    ) -> Result<AccountImport, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = self.deps.pool.begin().await?;
        let before = AccountImportRepo::get(&mut tx, scope, id).await?;
        if !AccountImportRepo::cancel(&mut tx, scope, id).await? {
            return Err(AccountError::ImportNotWaiting);
        }
        let staged = before.rows_total.saturating_sub(before.rows_done);
        let ended = Ended {
            outcome: "canceled",
            counts: BTreeMap::new(),
            rows: to_u32(staged),
        };
        self.record_import(&mut tx, scope, &before, &ended, audit)
            .await?;
        let import = AccountImportRepo::get(&mut tx, scope, id).await?;
        tx.commit().await?;
        Ok(import)
    }

    /// The `account_import` job's handler: one [`Self::run_import`] attempt. When the last
    /// attempt the queue allows fails, the import is failed and its staged rows deleted, so the
    /// owner sees it end instead of a progress bar that stops until the stale sweep.
    ///
    /// # Errors
    ///
    /// What [`Self::run_import`] returns.
    pub async fn run_import_attempt(
        &self,
        job: AccountImportJob,
        ctx: &JobContext,
    ) -> Result<(), JobError> {
        let result = self.run_import(job).await;
        if result.is_err() && ctx.is_last_attempt() {
            let owner = UserId::from_uuid(job.owner);
            let id = AccountImportId::from_uuid(job.import);
            let abandoned = match self.deps.pool.acquire().await {
                Ok(mut conn) => abandon_import(&mut conn, owner, id).await,
                Err(error) => Err(job_error(error)),
            };
            if let Err(error) = abandoned {
                // The stale sweep fails it later.
                tracing::warn!(import = %id.as_uuid(), %error, "a failed account import was not closed");
            }
        }
        result
    }

    /// The `account_import` job: checks the staged rows with the platform, connects them for a
    /// commit run, and settles each row in its own transaction, which also deletes the staged
    /// row and, for a commit run, connects it. A retry resumes at the first row not yet
    /// settled, and a second run of the same import at once settles each row only once. A run
    /// for an import that is gone or not `processing` changes nothing.
    ///
    /// # Errors
    ///
    /// [`JobError::Retry`] on a database failure; the row in hand stays staged, with nothing
    /// connected.
    pub async fn run_import(&self, job: AccountImportJob) -> Result<(), JobError> {
        let owner = UserId::from_uuid(job.owner);
        let id = AccountImportId::from_uuid(job.import);
        let mut conn = self.deps.pool.acquire().await.map_err(job_error)?;
        let import = match AccountImportRepo::fetch(&mut conn, owner, id).await {
            Ok(import) => import,
            Err(DataError::NotFound) => return Ok(()),
            Err(error) => return Err(job_error(error)),
        };
        if import.status != ImportState::Processing {
            return Ok(());
        }
        // The platform may have been switched off or lost its configuration since the upload.
        // The import cannot finish then, and its staged credentials are not kept.
        let resolved = match self.resolve(&import.platform).await {
            Ok(resolved) => resolved,
            Err(AccountError::Data(error)) => return Err(job_error(error)),
            Err(_) => return abandon_import(&mut conn, owner, id).await,
        };
        let ConnectMethod::Credentials(schema) = &resolved.agent.descriptor().connect else {
            return abandon_import(&mut conn, owner, id).await;
        };
        drop(conn);

        let scope = OwnerScope::own(owner);
        let concurrency = usize::from(self.deps.settings.csv_concurrency.max(1));
        let chunk_rows = i64::try_from(concurrency * 4).unwrap_or(4);
        let config = resolved.config.clone();
        let agent = resolved.agent;
        loop {
            let mut conn = self.deps.pool.acquire().await.map_err(job_error)?;
            let chunk = AccountImportRepo::next_chunk(&mut conn, owner, id, chunk_rows)
                .await
                .map_err(job_error)?;
            drop(conn);
            if chunk.is_empty() {
                break;
            }
            let row_nos: Vec<i32> = chunk.iter().map(|row| row.row_no).collect();

            // Check the chunk's rows concurrently, holding no database connection, then settle
            // them in file order.
            // Owned rows: a closure over a borrowed row cannot be inferred `Send` here.
            let checks = chunk
                .into_iter()
                .map(|staged| self.check_staged(agent, schema, staged, &config));
            let checked: Vec<Checked> = futures::stream::iter(checks)
                .buffered(concurrency)
                .collect()
                .await;
            let mut settled = 0;
            for (row_no, row) in row_nos.into_iter().zip(checked) {
                if self
                    .settle_row(&scope, &import, row_no, row)
                    .await
                    .map_err(job_error)?
                {
                    settled += 1;
                }
            }
            if settled == 0 {
                // Another run of this import settled the chunk first, or the import stopped
                // being `processing`; the run that settles the last row closes it.
                break;
            }
        }

        let mut tx = self.deps.pool.begin().await.map_err(job_error)?;
        if !AccountImportRepo::finish(&mut tx, owner, id)
            .await
            .map_err(job_error)?
        {
            return Ok(());
        }
        let done = AccountImportRepo::fetch(&mut tx, owner, id)
            .await
            .map_err(job_error)?;
        let ended = Ended {
            outcome: "completed",
            counts: count_statuses(&done.report),
            rows: to_u32(done.rows_done),
        };
        self.record_import(&mut tx, &scope, &done, &ended, done.request)
            .await
            .map_err(job_error)?;
        tx.commit().await.map_err(job_error)?;
        Ok(())
    }

    /// Opens one staged row and asks the platform about it. Holds no database connection.
    async fn check_staged(
        &self,
        agent: &dyn postit_core::Agent,
        schema: &OptionsSchema,
        staged: StagedAccountRow,
        config: &Value,
    ) -> Checked {
        let row = to_u32(staged.row_no);
        let line = to_u32(staged.line_no);
        let opened = self
            .deps
            .vault
            .open(&staged.data, &AccountImportRepo::seal_context(staged.id));
        match opened {
            Ok(StagedCells::Cells {
                cells,
                display_name,
            }) => {
                self.check_row(agent, schema, &cells, display_name, (row, line), config)
                    .await
            }
            Ok(StagedCells::Malformed) => Checked {
                row,
                line,
                outcome: Err(malformed_row(row, line)),
            },
            Err(error) => {
                tracing::warn!(%error, "a staged import row did not open");
                Checked {
                    row,
                    line,
                    outcome: Err(malformed_row(row, line)),
                }
            }
        }
    }

    /// Parses one row and asks the platform about it. Holds no database connection.
    async fn check_row(
        &self,
        agent: &dyn postit_core::Agent,
        schema: &OptionsSchema,
        cells: &[(String, String)],
        display_name: Option<String>,
        (row, line): (u32, u32),
        config: &Value,
    ) -> Checked {
        let mut values = Map::new();
        for (name, cell) in cells {
            values.insert(name.clone(), schema.cell_value(name, cell));
        }

        let mut bad = row_report(row, line, ImportStatus::InvalidFields);
        if display_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > DISPLAY_NAME_MAX)
        {
            bad.violations.push("option_too_long".to_owned());
            bad.field = Some(DISPLAY_NAME.to_owned());
            return Checked {
                row,
                line,
                outcome: Err(bad),
            };
        }
        let fields = match schema.parse_credentials(values) {
            Ok(fields) => fields,
            Err(violations) => {
                let violations = redact_violations(violations);
                bad.field = violations.first().and_then(field_of);
                bad.violations = violations
                    .iter()
                    .map(|v| v.code.as_str().to_owned())
                    .collect();
                return Checked {
                    row,
                    line,
                    outcome: Err(bad),
                };
            }
        };

        let found = agent
            .verify_credentials(&self.check_context(config.clone()), &fields)
            .await;
        let outcome = match found {
            Ok(discovered) if discovered.accounts.is_empty() => {
                Err(row_report(row, line, ImportStatus::NothingFound))
            }
            Ok(discovered) => Ok(Verified {
                fields,
                discovered: discovered.accounts,
                display_name,
            }),
            Err(error) => Err(failure_report(row, line, &error)),
        };
        Checked { row, line, outcome }
    }

    /// Settles staged row `row_no` in one transaction: takes it, classifies it against the
    /// database, connects it for a commit run, and records its report entry. Returns `false`,
    /// changing nothing, when another run settled the row first or the import is no longer
    /// `processing`. On an error nothing is kept: the row stays staged and nothing is
    /// connected.
    async fn settle_row(
        &self,
        scope: &OwnerScope,
        import: &AccountImport,
        row_no: i32,
        checked: Checked,
    ) -> Result<bool, AccountError> {
        let mut tx = self.deps.pool.begin().await?;
        let Some(seen) =
            AccountImportRepo::claim_row(&mut tx, scope.owner, import.id, row_no).await?
        else {
            return Ok(false);
        };
        let seen: HashSet<String> = seen.into_iter().collect();
        let (report, fresh) = self
            .settle_checked(&mut tx, scope, import, checked, &seen)
            .await?;
        let entry =
            serde_json::to_value(&report).map_err(|e| AccountError::Internal(e.to_string()))?;
        AccountImportRepo::settle_row(&mut tx, scope.owner, import.id, &entry, &fresh).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Classifies a checked row against the database and, for a commit run, connects it, in the
    /// caller's transaction. Returns the row's report and the remote IDs it adds to `seen`.
    async fn settle_checked(
        &self,
        tx: &mut PgConnection,
        scope: &OwnerScope,
        import: &AccountImport,
        checked: Checked,
        seen: &HashSet<String>,
    ) -> Result<(ImportRow, Vec<String>), AccountError> {
        let platform = &import.platform;
        let (row, line) = (checked.row, checked.line);
        let verified = match checked.outcome {
            Ok(verified) => verified,
            Err(report) => return Ok((report, Vec::new())),
        };

        let mut elsewhere = 0;
        let mut own = 0;
        for found in &verified.discovered {
            match AccountRepo::remote_account(&mut *tx, scope, platform, &found.remote_id).await? {
                RemoteAccount::Elsewhere => elsewhere += 1,
                RemoteAccount::Own { active: true, .. } => own += 1,
                RemoteAccount::Own { active: false, .. } | RemoteAccount::Free => {
                    if seen.contains(&found.remote_id) {
                        own += 1;
                    }
                }
            }
        }

        let total = verified.discovered.len();
        let mut report = row_report(row, line, ImportStatus::Ok);
        if elsewhere == total {
            report.status = ImportStatus::ConnectedElsewhere;
            return Ok((report, Vec::new()));
        }
        if own + elsewhere == total && own > 0 {
            report.status = ImportStatus::AlreadyConnected;
        }

        let single = total == 1;
        let chosen_name = if single {
            verified.display_name.as_deref()
        } else {
            None
        };
        report.display_name_ignored = verified.display_name.is_some() && !single;

        let fresh: Vec<String> = verified
            .discovered
            .iter()
            .map(|found| found.remote_id.clone())
            .filter(|remote| !seen.contains(remote))
            .collect();
        if !import.commit {
            return Ok((report, fresh));
        }

        let (plain, secret) = verified.fields.split();
        // A savepoint, so a row that cannot be connected still settles.
        let mut connect = tx.begin().await?;
        let connected = self
            .connect_in_tx(
                &mut connect,
                scope,
                ConnectInput::credentials(
                    platform,
                    serde_json::json!(secret),
                    Value::Object(plain),
                    verified.discovered.iter().collect(),
                    chosen_name,
                ),
                import.request,
            )
            .await;
        match connected {
            Ok(done) => {
                connect.commit().await?;
                report.account_ids = done.connected.iter().map(|(a, _)| a.id).collect();
                Ok((report, fresh))
            }
            Err(AccountError::SendingMethodExists) => {
                // The row cannot be connected; the rest of the file still can.
                connect.rollback().await?;
                report.status = ImportStatus::AlreadyConnected;
                Ok((report, fresh))
            }
            Err(error) => Err(error),
        }
    }

    /// Writes the single `accounts_imported` audit event for an import that finished or was
    /// canceled, in the caller's transaction.
    async fn record_import(
        &self,
        conn: &mut PgConnection,
        scope: &OwnerScope,
        import: &AccountImport,
        ended: &Ended<'_>,
        audit: AuditContext,
    ) -> Result<(), AccountError> {
        let counts: Map<String, Value> = ended
            .counts
            .iter()
            .map(|(status, n)| (status.clone(), Value::from(*n)))
            .collect();
        let stopped = import
            .stopped
            .map_or(Value::Null, |stop| Value::from(stop.as_str()));
        let event = AuditEvent::new(AuditEventKind::AccountsImported)
            .actor(scope.actor)
            .owner(scope.owner)
            .context(audit)
            .detail("commit", import.commit)
            .and_then(|e| e.detail("platform", import.platform.as_str()))
            .and_then(|e| e.detail("outcome", ended.outcome))
            .and_then(|e| e.detail("rows", ended.rows))
            .and_then(|e| e.detail("stopped", stopped))
            .and_then(|e| e.detail("counts", Value::Object(counts)))
            .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
        AuditLog::record(conn, AuditEventId::from(self.deps.ids.generate()), event).await?;
        Ok(())
    }
}

/// Fails an import that can no longer run and deletes its staged rows.
async fn abandon_import(
    conn: &mut PgConnection,
    owner: UserId,
    id: AccountImportId,
) -> Result<(), JobError> {
    tracing::warn!(import = %id.as_uuid(), "an account import cannot run and is failed");
    AccountImportRepo::abandon(conn, owner, id)
        .await
        .map_err(job_error)
}

/// How many report entries ended each way, by status name.
fn count_statuses(report: &Value) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    for entry in report.as_array().into_iter().flatten() {
        if let Some(status) = entry.get("status").and_then(Value::as_str) {
            *counts.entry(status.to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

/// Reads and checks the header row: only schema fields and `display_name`, each once, with
/// every required field present. Names columns, never values.
async fn read_headers<R>(
    reader: &mut csv_async::AsyncReader<R>,
    schema: &OptionsSchema,
) -> Result<Vec<String>, AccountError>
where
    R: AsyncRead + Unpin + Send,
{
    let record = reader
        .headers()
        .await
        .map_err(|_| AccountError::CsvInvalid("the header row is unreadable".to_owned()))?;
    let headers: Vec<String> = record
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let h = if i == 0 {
                h.trim_start_matches('\u{feff}')
            } else {
                h
            };
            h.trim().to_owned()
        })
        .collect();
    let mut known = HashSet::new();
    for name in &headers {
        let allowed = name == DISPLAY_NAME || schema.fields().iter().any(|f| &f.name == name);
        if !allowed {
            return Err(AccountError::CsvInvalid(format!("unknown column `{name}`")));
        }
        if !known.insert(name.as_str()) {
            return Err(AccountError::CsvInvalid(format!(
                "duplicate column `{name}`"
            )));
        }
    }
    for field in schema.fields() {
        if field.required && !known.contains(field.name.as_str()) {
            return Err(AccountError::CsvInvalid(format!(
                "missing column `{}`",
                field.name
            )));
        }
    }
    Ok(headers)
}

fn field_of(violation: &Violation) -> Option<String> {
    violation
        .path
        .as_deref()
        .map(|p| p.strip_prefix("options.").unwrap_or(p).to_owned())
}

fn failure_report(row: u32, line: u32, error: &AgentError) -> ImportRow {
    match error {
        AgentError::InvalidCredentials { field } => {
            let mut report = row_report(row, line, ImportStatus::InvalidCredentials);
            report.field.clone_from(field);
            report
        }
        AgentError::Authentication { .. } | AgentError::Authorization { .. } => {
            row_report(row, line, ImportStatus::InvalidCredentials)
        }
        other => {
            tracing::warn!(
                error = crate::error::agent_error_name(other),
                "import row failed"
            );
            row_report(row, line, ImportStatus::PlatformError)
        }
    }
}
