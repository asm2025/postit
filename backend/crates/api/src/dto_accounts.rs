//! Wire types of the account routes. None carries a credential, a connection config, or a
//! sealed value: the DTOs are built field by field from the service's results.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_core::DiscoveredAccount;
use postit_data::account_imports::{AccountImport, ImportState, ImportStop};
use postit_data::accounts::{Account, AccountStatus};
use postit_oauth::{
    AccountChange, AccountView, Confirmed, ConnectKind, CredentialCheck, DisconnectOutcome,
    ImportRow, ImportStatus, OAuthStart, PendingSummary, PendingView,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;

/// Whether an account can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatusDto {
    /// Usable.
    Active,
    /// The credential stopped working; reconnect it.
    NeedsReconnect,
    /// Access was revoked.
    Revoked,
    /// The owner disconnected it; the row stays for history.
    Disconnected,
}

impl From<AccountStatus> for AccountStatusDto {
    fn from(status: AccountStatus) -> Self {
        match status {
            AccountStatus::Active => Self::Active,
            AccountStatus::NeedsReconnect => Self::NeedsReconnect,
            AccountStatus::Revoked => Self::Revoked,
            AccountStatus::Disconnected => Self::Disconnected,
        }
    }
}

/// How an account is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectKindDto {
    /// With credentials the user pasted.
    Credentials,
    /// With an OAuth sign-in.
    Oauth,
}

impl From<ConnectKind> for ConnectKindDto {
    fn from(kind: ConnectKind) -> Self {
        match kind {
            ConnectKind::Credentials => Self::Credentials,
            ConnectKind::OAuth => Self::Oauth,
        }
    }
}

/// A connected account.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountDto {
    /// Account ID.
    pub id: Uuid,
    /// The platform's ID, such as `mastodon`.
    pub platform: String,
    /// The platform's ID for the account.
    pub remote_account_id: String,
    /// The name shown in postit.
    pub display_name: String,
    /// Profile picture, when the platform gives one.
    pub avatar_url: Option<String>,
    /// Whether the account can be used.
    pub status: AccountStatusDto,
    /// Whether publishing to it needs no live confirmation.
    pub test_account: bool,
    /// How it is connected; absent when this build has no agent for the platform.
    pub connect: Option<ConnectKindDto>,
    /// When the connection was last verified.
    pub last_checked_at: Option<DateTime<Utc>>,
    /// When it was connected.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl AccountDto {
    fn new(account: &Account, connect: Option<ConnectKind>) -> Self {
        Self {
            id: account.id.as_uuid(),
            platform: account.platform.as_str().to_owned(),
            remote_account_id: account.remote_account_id.clone(),
            display_name: account.display_name.clone(),
            avatar_url: account.avatar_url.clone(),
            status: account.status.into(),
            test_account: account.test_account,
            connect: connect.map(Into::into),
            last_checked_at: account.last_checked_at,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

impl From<AccountView> for AccountDto {
    fn from(view: AccountView) -> Self {
        Self::new(&view.account, view.connect)
    }
}

/// An account a pasted credential or an OAuth sign-in reaches.
#[derive(Debug, Serialize, ToSchema)]
pub struct DiscoveredAccountDto {
    /// The platform's ID for the account. Pass it to confirm to pick this account.
    pub remote_id: String,
    /// Name shown to people.
    pub display_name: String,
    /// The `@handle`, when the platform has one.
    pub handle: Option<String>,
    /// Profile picture.
    pub avatar_url: Option<String>,
}

impl From<DiscoveredAccount> for DiscoveredAccountDto {
    fn from(found: DiscoveredAccount) -> Self {
        Self {
            remote_id: found.remote_id,
            display_name: found.display_name,
            handle: found.handle,
            avatar_url: found.avatar_url.map(|u| u.to_string()),
        }
    }
}

/// The result of checking a pasted credential.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialCheckResponse {
    /// Confirm with this ID. Shown once; absent when the credential reaches no account.
    pub connect_id: Option<String>,
    /// The row to cancel the check by (`DELETE /accounts/pending-connections/{id}`); absent
    /// with `connect_id`. It cannot confirm.
    pub pending_id: Option<Uuid>,
    /// When `connect_id` stops working.
    pub expires_at: Option<DateTime<Utc>>,
    /// The accounts the credential reaches.
    pub discovered: Vec<DiscoveredAccountDto>,
}

impl From<CredentialCheck> for CredentialCheckResponse {
    fn from(check: CredentialCheck) -> Self {
        let (connect_id, pending_id, expires_at) = match check.pending {
            Some((id, row, at)) => (Some(id.as_str().to_owned()), Some(row.as_uuid()), Some(at)),
            None => (None, None, None),
        };
        Self {
            connect_id,
            pending_id,
            expires_at,
            discovered: check.discovered.into_iter().map(Into::into).collect(),
        }
    }
}

/// Where the browser goes back to once the platform's sign-in is over.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartOAuthRequest {
    /// Exactly one of the deployment's allowed return URLs: the web app's `/oauth-done`, the
    /// mobile app's `<scheme>://oauth-done`, or a configured entry. No prefix match, query, or
    /// fragment.
    pub return_to: String,
}

/// A started OAuth connection.
#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthStartResponse {
    /// Send the user's browser here to sign in to the platform. It carries a one-time `state`.
    pub authorize_url: String,
    /// When the sign-in must be finished by.
    pub expires_at: DateTime<Utc>,
}

impl From<OAuthStart> for OAuthStartResponse {
    fn from(start: OAuthStart) -> Self {
        Self {
            authorize_url: start.authorize_url.into(),
            expires_at: start.expires_at,
        }
    }
}

/// A connection waiting for the user to pick accounts, from pasted credentials or an OAuth
/// sign-in.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingConnectionDto {
    /// The row ID to cancel the connection by (`DELETE /accounts/pending-connections/{id}`).
    /// It cannot confirm.
    pub pending_id: Uuid,
    /// The platform's ID.
    pub platform: String,
    /// How the accounts would be connected.
    pub connect: ConnectKindDto,
    /// When the connection can no longer be confirmed.
    pub expires_at: DateTime<Utc>,
    /// The accounts it reaches; confirm with the `remote_id`s to connect.
    pub discovered: Vec<DiscoveredAccountDto>,
}

impl From<PendingView> for PendingConnectionDto {
    fn from(view: PendingView) -> Self {
        Self {
            pending_id: view.pending_id,
            platform: view.platform.as_str().to_owned(),
            connect: view.connect.into(),
            expires_at: view.expires_at,
            discovered: view.discovered.into_iter().map(Into::into).collect(),
        }
    }
}

/// An unfinished credential check in the owner's list. It shows no accounts, no account
/// count, and no `connect_id`, and it cannot be confirmed: only the client that holds the
/// `connect_id` can confirm. A lost `connect_id` means starting the connection again; cancel
/// this entry and connect again. OAuth sign-ins are never listed.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingSummaryDto {
    /// The row ID. Cancel with `DELETE /accounts/pending-connections/{id}`.
    pub pending_id: Uuid,
    /// The platform's ID.
    pub platform: String,
    /// When the credential check ran.
    pub created_at: DateTime<Utc>,
    /// When the connection can no longer be confirmed.
    pub expires_at: DateTime<Utc>,
}

impl From<PendingSummary> for PendingSummaryDto {
    fn from(row: PendingSummary) -> Self {
        Self {
            pending_id: row.pending_id,
            platform: row.platform.as_str().to_owned(),
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}

/// Which accounts to connect.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmRequest {
    /// The `remote_id` of each account to connect; omit to connect every discovered account.
    pub account_ids: Option<Vec<String>>,
}

/// What confirming did to one account.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountChangeDto {
    /// A new account.
    Created,
    /// A disconnected account is active again.
    Reactivated,
    /// An account moved from an older credential to this one.
    Moved,
}

impl From<AccountChange> for AccountChangeDto {
    fn from(change: AccountChange) -> Self {
        match change {
            AccountChange::Created => Self::Created,
            AccountChange::Reactivated => Self::Reactivated,
            AccountChange::Moved => Self::Moved,
        }
    }
}

/// One connected account and what happened to it.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectedAccountDto {
    /// The account.
    pub account: AccountDto,
    /// What confirming did.
    pub change: AccountChangeDto,
}

/// The result of confirming a connection.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmResponse {
    /// The accounts now connected.
    pub connected: Vec<ConnectedAccountDto>,
    /// How many chosen accounts someone else already has connected.
    pub skipped_elsewhere: u32,
    /// Chosen IDs the credential did not reach.
    pub unknown: Vec<String>,
}

impl From<Confirmed> for ConfirmResponse {
    fn from(confirmed: Confirmed) -> Self {
        let connect = confirmed.connect;
        Self {
            connected: confirmed
                .connected
                .into_iter()
                .map(|(account, change)| ConnectedAccountDto {
                    account: AccountDto::new(&account, Some(connect)),
                    change: change.into(),
                })
                .collect(),
            skipped_elsewhere: confirmed.skipped_elsewhere,
            unknown: confirmed.unknown,
        }
    }
}

/// Settings the owner may change on an account.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAccountRequest {
    /// Mark or unmark the account as a test account.
    pub test_account: Option<bool>,
    /// A new name, 1-100 characters.
    pub display_name: Option<String>,
}

/// An email to send once, as a test, to the signed-in user's own address.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestEmailRequest {
    /// The document, saved or not.
    pub email: postit_core::EmailDocument,
}

/// What the sending domain's DNS records look like. Each finding is a fixed code the clients
/// translate; no domain, selector, or record text is returned.
#[derive(Debug, Serialize, ToSchema)]
pub struct DnsCheckResponse {
    /// One finding each for SPF, DKIM, and DMARC, in that order.
    pub findings: Vec<postit_core::DomainFinding>,
}

/// What disconnecting an account did.
#[derive(Debug, Serialize, ToSchema)]
pub struct DisconnectResponse {
    /// Pending and parked deliveries that were canceled.
    pub canceled_deliveries: u32,
    /// Deliveries still held for a later time on the platform's side.
    pub native_scheduled_remaining: u32,
}

impl From<DisconnectOutcome> for DisconnectResponse {
    fn from(outcome: DisconnectOutcome) -> Self {
        Self {
            canceled_deliveries: outcome.canceled_deliveries,
            native_scheduled_remaining: outcome.native_scheduled_remaining,
        }
    }
}

/// How one CSV row ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatusDto {
    /// The credential works and its accounts can be (or were) connected.
    Ok,
    /// The values do not fit the credentials schema.
    InvalidFields,
    /// The platform rejected the credential.
    InvalidCredentials,
    /// Every account the credential reaches is already yours.
    AlreadyConnected,
    /// Every account the credential reaches belongs to someone else.
    ConnectedElsewhere,
    /// The credential reaches no account.
    NothingFound,
    /// The platform could not answer.
    PlatformError,
}

impl From<ImportStatus> for ImportStatusDto {
    fn from(status: ImportStatus) -> Self {
        match status {
            ImportStatus::Ok => Self::Ok,
            ImportStatus::InvalidFields => Self::InvalidFields,
            ImportStatus::InvalidCredentials => Self::InvalidCredentials,
            ImportStatus::AlreadyConnected => Self::AlreadyConnected,
            ImportStatus::ConnectedElsewhere => Self::ConnectedElsewhere,
            ImportStatus::NothingFound => Self::NothingFound,
            ImportStatus::PlatformError => Self::PlatformError,
        }
    }
}

/// The report line for one data row. Never carries a field value.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportRowDto {
    /// The 1-based data row number.
    pub row: u32,
    /// The file line the record starts on; a quoted cell with line breaks makes it larger than
    /// `row + 1`.
    pub line: u32,
    /// How the row ended.
    pub status: ImportStatusDto,
    /// The violation codes, for `invalid_fields`.
    pub violations: Vec<String>,
    /// The credential field at fault, when known.
    pub field: Option<String>,
    /// The accounts created or matched; empty on a dry run.
    pub account_ids: Vec<Uuid>,
    /// A `display_name` was given but the row reaches more than one account.
    pub display_name_ignored: bool,
}

impl From<ImportRow> for ImportRowDto {
    fn from(row: ImportRow) -> Self {
        Self {
            row: row.row,
            line: row.line,
            status: row.status.into(),
            violations: row.violations,
            field: row.field,
            account_ids: row.account_ids.into_iter().map(|a| a.as_uuid()).collect(),
            display_name_ignored: row.display_name_ignored,
        }
    }
}

/// Where an account import is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStateDto {
    /// Rows are being checked, or are about to be.
    Processing,
    /// A commit run whose file stopped early: nothing is connected until the owner continues or
    /// cancels.
    AwaitingDecision,
    /// Every row was settled.
    Completed,
    /// The import did not finish.
    Failed,
    /// The owner canceled it while it waited; nothing was connected.
    Canceled,
}

impl From<ImportState> for ImportStateDto {
    fn from(state: ImportState) -> Self {
        match state {
            ImportState::Processing => Self::Processing,
            ImportState::AwaitingDecision => Self::AwaitingDecision,
            ImportState::Completed => Self::Completed,
            ImportState::Failed => Self::Failed,
            ImportState::Canceled => Self::Canceled,
        }
    }
}

/// Why reading the file stopped before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStopDto {
    /// The next row would have passed the deployment's row limit.
    MaxRows,
    /// The file went over the deployment's size limit.
    MaxBytes,
    /// The upload could not be read to its end.
    Unreadable,
}

impl From<ImportStop> for ImportStopDto {
    fn from(stop: ImportStop) -> Self {
        match stop {
            ImportStop::MaxRows => Self::MaxRows,
            ImportStop::MaxBytes => Self::MaxBytes,
            ImportStop::Unreadable => Self::Unreadable,
        }
    }
}

/// An account import: its progress and the report so far, in file order. Never carries a field
/// value.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountImportDto {
    /// The import.
    pub id: Uuid,
    /// The platform the accounts are for.
    pub platform: String,
    /// Whether accounts are connected (`true`) or the file is only checked.
    pub commit: bool,
    /// Where it is.
    pub status: ImportStateDto,
    /// How many rows were read from the file.
    pub rows_total: i32,
    /// How many rows are settled and in `rows`.
    pub rows_done: i32,
    /// Why reading stopped before the end of the file, when it did. The rows read whole before
    /// the stop are the import.
    pub stopped: Option<ImportStopDto>,
    /// The last data row read, or 0.
    pub last_row: i32,
    /// The file line that row ends on, or 0; a quoted cell with line breaks ends it past the
    /// line it starts on. A user whose file stopped early can remove the lines up to it and
    /// import the rest.
    pub last_line: i32,
    /// How many settled rows ended each way, keyed by status.
    pub counts: BTreeMap<ImportStatusDto, u32>,
    /// One entry per settled row.
    pub rows: Vec<ImportRowDto>,
}

impl TryFrom<AccountImport> for AccountImportDto {
    type Error = ApiError;

    fn try_from(import: AccountImport) -> Result<Self, ApiError> {
        let rows: Vec<ImportRow> =
            serde_json::from_value(import.report).map_err(|e| ApiError::internal(&e))?;
        let mut counts = BTreeMap::new();
        for row in &rows {
            *counts.entry(row.status.into()).or_insert(0) += 1;
        }
        Ok(Self {
            id: import.id.as_uuid(),
            platform: import.platform.to_string(),
            commit: import.commit,
            status: import.status.into(),
            rows_total: import.rows_total,
            rows_done: import.rows_done,
            stopped: import.stopped.map(Into::into),
            last_row: import.last_row,
            last_line: import.last_line,
            counts,
            rows: rows.into_iter().map(Into::into).collect(),
        })
    }
}
