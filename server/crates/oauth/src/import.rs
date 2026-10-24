//! Importing credential accounts from a CSV file.
//!
//! The body is read as a stream and checked row by row. Rows are verified with the platform up
//! to `accounts.csv_concurrency` at a time, then classified and committed one at a time in
//! file order, so a remote account that appears twice reports `already_connected` for its
//! second row. Nothing is written to disk and a row is dropped once its report entry exists.

use std::collections::{BTreeMap, HashSet};

use csv_async::{AsyncReaderBuilder, StringRecord, Trim};
use futures::StreamExt as _;
use postit_core::{
    AccountId, AgentError, AuditEventId, ConnectMethod, CredentialFields, DiscoveredAccount,
    OptionsSchema, PlatformId, Violation,
};
use postit_data::accounts::{AccountRepo, RemoteAccount};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::{Capability, DataError, OwnerScope};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::io::AsyncRead;

use crate::connect::{ConnectInput, redact_violations};
use crate::{AccountError, AccountService};

/// The optional column that names the account when a row reaches exactly one.
const DISPLAY_NAME: &str = "display_name";
/// The longest display name an account may be given.
const DISPLAY_NAME_MAX: usize = 100;

/// How one CSV row ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct ImportRow {
    /// The 1-based data row number.
    pub row: u32,
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

/// The result of an import, in file order.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// Whether accounts were connected (`true`) or the file was only checked (`false`).
    pub commit: bool,
    /// One entry per data row.
    pub rows: Vec<ImportRow>,
    /// How many rows ended each way.
    pub counts: BTreeMap<ImportStatus, u32>,
    /// The file has more rows than `accounts.csv_max_rows`; the rest were not read.
    pub truncated: bool,
}

/// What the row stream yields for the consumer to settle in file order.
enum Item {
    Row(Checked),
    /// The file went past `accounts.csv_max_rows`.
    Truncated,
    /// The body failed while being read.
    Unreadable,
}

/// A row after verification, before it is classified or committed.
struct Checked {
    row: u32,
    outcome: Result<Verified, ImportRow>,
}

struct Verified {
    fields: CredentialFields,
    discovered: Vec<DiscoveredAccount>,
    display_name: Option<String>,
}

/// A row the CSV reader could not parse (for example the wrong number of columns).
fn malformed_row(row: u32) -> ImportRow {
    let mut report = row_report(row, ImportStatus::InvalidFields);
    report.violations.push("malformed_row".to_owned());
    report
}

fn row_report(row: u32, status: ImportStatus) -> ImportRow {
    ImportRow {
        row,
        status,
        violations: Vec::new(),
        field: None,
        account_ids: Vec::new(),
        display_name_ignored: false,
    }
}

impl AccountService {
    /// Checks every row of a CSV of credentials and, with `commit`, connects the accounts each
    /// valid row reaches.
    ///
    /// The header row lists the credentials schema's field names and may add `display_name`.
    /// A row that fails, malformed ones included, never stops the others. Reading stops at
    /// `accounts.csv_max_rows` rows and the report says it was `truncated`; the rows before
    /// that point are processed as usual.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, the catalog's errors,
    /// [`AccountError::ConnectMethodUnsupported`] for an OAuth platform,
    /// [`AccountError::CsvInvalid`] for a bad header or a body that cannot be read.
    pub async fn import_csv<R>(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        body: R,
        commit: bool,
        audit: AuditContext,
    ) -> Result<ImportReport, AccountError>
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

        let max_rows = self.deps.settings.csv_max_rows;
        let concurrency = usize::from(self.deps.settings.csv_concurrency.max(1));
        let schema_ref = schema;
        let config = resolved.config.clone();
        let agent = resolved.agent;

        // Rows are pulled one at a time: at most `concurrency` are in memory, and the first
        // starts verifying before the rest of the body has arrived.
        let headers_ref = &headers;
        let items = reader
            .into_records()
            .enumerate()
            .map(|(index, record)| {
                let row = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let config = config.clone();
                async move {
                    if row > max_rows {
                        return Item::Truncated;
                    }
                    match record {
                        Ok(record) => Item::Row(
                            self.check_row(agent, schema_ref, headers_ref, &record, row, &config)
                                .await,
                        ),
                        // A read failure repeats on every poll; only a bad row is skippable.
                        Err(error) if matches!(error.kind(), csv_async::ErrorKind::Io(_)) => {
                            Item::Unreadable
                        }
                        Err(_) => Item::Row(Checked {
                            row,
                            outcome: Err(malformed_row(row)),
                        }),
                    }
                }
            })
            .buffered(concurrency);
        futures::pin_mut!(items);

        let mut rows = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut truncated = false;
        while let Some(item) = items.next().await {
            match item {
                Item::Row(checked) => rows.push(
                    self.settle_row(scope, platform, checked, commit, &mut seen, audit)
                        .await?,
                ),
                Item::Truncated => {
                    truncated = true;
                    break;
                }
                Item::Unreadable => {
                    return Err(AccountError::CsvInvalid(
                        "the file could not be read".to_owned(),
                    ));
                }
            }
        }

        let mut counts = BTreeMap::new();
        for row in &rows {
            *counts.entry(row.status).or_insert(0) += 1;
        }
        let report = ImportReport {
            commit,
            rows,
            counts,
            truncated,
        };
        self.record_import(scope, platform, &report, audit).await?;
        Ok(report)
    }

    /// Parses one row and asks the platform about it. Holds no database connection.
    async fn check_row(
        &self,
        agent: &dyn postit_core::Agent,
        schema: &OptionsSchema,
        headers: &[String],
        record: &StringRecord,
        row: u32,
        config: &Value,
    ) -> Checked {
        let mut values = Map::new();
        let mut display_name = None;
        for (name, cell) in headers.iter().zip(record.iter()) {
            let cell = cell.trim();
            if cell.is_empty() {
                continue;
            }
            if name == DISPLAY_NAME {
                display_name = Some(cell.to_owned());
            } else {
                values.insert(name.clone(), Value::String(cell.to_owned()));
            }
        }

        let mut bad = row_report(row, ImportStatus::InvalidFields);
        if display_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > DISPLAY_NAME_MAX)
        {
            bad.violations.push("option_too_long".to_owned());
            bad.field = Some(DISPLAY_NAME.to_owned());
            return Checked {
                row,
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
                    outcome: Err(bad),
                };
            }
        };

        let found = agent
            .verify_credentials(&self.check_context(config.clone()), &fields)
            .await;
        match found {
            Ok(discovered) if discovered.accounts.is_empty() => Checked {
                row,
                outcome: Err(row_report(row, ImportStatus::NothingFound)),
            },
            Ok(discovered) => Checked {
                row,
                outcome: Ok(Verified {
                    fields,
                    discovered: discovered.accounts,
                    display_name,
                }),
            },
            Err(error) => Checked {
                row,
                outcome: Err(failure_report(row, &error)),
            },
        }
    }

    /// Classifies a verified row against the database and, when committing, connects it.
    async fn settle_row(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        checked: Checked,
        commit: bool,
        seen: &mut HashSet<String>,
        audit: AuditContext,
    ) -> Result<ImportRow, AccountError> {
        let row = checked.row;
        let verified = match checked.outcome {
            Ok(verified) => verified,
            Err(report) => return Ok(report),
        };

        let mut conn = self.deps.pool.acquire().await?;
        let mut elsewhere = 0;
        let mut own = 0;
        for found in &verified.discovered {
            match AccountRepo::remote_account(&mut conn, scope, platform, &found.remote_id).await? {
                RemoteAccount::Elsewhere => elsewhere += 1,
                RemoteAccount::Own { active: true, .. } => own += 1,
                RemoteAccount::Own { active: false, .. } | RemoteAccount::Free => {
                    if seen.contains(&found.remote_id) {
                        own += 1;
                    }
                }
            }
        }
        drop(conn);

        let total = verified.discovered.len();
        let mut report = row_report(row, ImportStatus::Ok);
        if elsewhere == total {
            report.status = ImportStatus::ConnectedElsewhere;
            return Ok(report);
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

        for found in &verified.discovered {
            seen.insert(found.remote_id.clone());
        }
        if !commit {
            return Ok(report);
        }

        let (plain, secret) = verified.fields.split();
        let mut tx = self.deps.pool.begin().await?;
        let connected = self
            .connect_in_tx(
                &mut tx,
                scope,
                ConnectInput {
                    platform,
                    secrets: serde_json::json!(secret),
                    config: Value::Object(plain),
                    accounts: verified.discovered.iter().collect(),
                    display_name: chosen_name,
                },
                audit,
            )
            .await;
        match connected {
            Ok(done) => {
                tx.commit().await?;
                report.account_ids = done.connected.iter().map(|(a, _)| a.id).collect();
                Ok(report)
            }
            Err(AccountError::SendingMethodExists) => {
                // The row cannot be connected; the rest of the file still can.
                drop(tx);
                report.status = ImportStatus::AlreadyConnected;
                Ok(report)
            }
            Err(error) => Err(error),
        }
    }

    /// Writes the single `accounts_imported` audit event for a finished import.
    async fn record_import(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        report: &ImportReport,
        audit: AuditContext,
    ) -> Result<(), AccountError> {
        let counts: Map<String, Value> = report
            .counts
            .iter()
            .map(|(status, n)| {
                let name = serde_json::to_value(status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                (name, Value::from(*n))
            })
            .collect();
        let event = AuditEvent::new(AuditEventKind::AccountsImported)
            .actor(scope.actor)
            .owner(scope.owner)
            .context(audit)
            .detail("commit", report.commit)
            .and_then(|e| e.detail("platform", platform.as_str()))
            .and_then(|e| e.detail("counts", Value::Object(counts)))
            .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
        let mut conn = self.deps.pool.acquire().await?;
        AuditLog::record(
            &mut conn,
            AuditEventId::from(self.deps.ids.generate()),
            event,
        )
        .await?;
        Ok(())
    }
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

fn failure_report(row: u32, error: &AgentError) -> ImportRow {
    match error {
        AgentError::InvalidCredentials { field } => {
            let mut report = row_report(row, ImportStatus::InvalidCredentials);
            report.field.clone_from(field);
            report
        }
        AgentError::Authentication { .. } | AgentError::Authorization { .. } => {
            row_report(row, ImportStatus::InvalidCredentials)
        }
        other => {
            tracing::warn!(
                error = crate::error::agent_error_name(other),
                "import row failed"
            );
            row_report(row, ImportStatus::PlatformError)
        }
    }
}
