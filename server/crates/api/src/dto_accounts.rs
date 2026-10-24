//! Wire types of the account routes. None carries a credential, a connection config, or a
//! sealed value: the DTOs are built field by field from the service's results.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_core::DiscoveredAccount;
use postit_data::accounts::{Account, AccountStatus};
use postit_oauth::{
    AccountChange, AccountView, Confirmed, ConnectKind, CredentialCheck, DisconnectOutcome,
    ImportReport, ImportRow, ImportStatus,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

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

/// An account a pasted credential reaches.
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
    /// When `connect_id` stops working.
    pub expires_at: Option<DateTime<Utc>>,
    /// The accounts the credential reaches.
    pub discovered: Vec<DiscoveredAccountDto>,
}

impl From<CredentialCheck> for CredentialCheckResponse {
    fn from(check: CredentialCheck) -> Self {
        let (connect_id, expires_at) = match check.pending {
            Some((id, at)) => (Some(id.as_str().to_owned()), Some(at)),
            None => (None, None),
        };
        Self {
            connect_id,
            expires_at,
            discovered: check.discovered.into_iter().map(Into::into).collect(),
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

/// What disconnecting an account did.
#[derive(Debug, Serialize, ToSchema)]
pub struct DisconnectResponse {
    /// Pending and parked deliveries that were cancelled.
    pub cancelled_deliveries: u32,
    /// Deliveries still held for a later time on the platform's side.
    pub native_scheduled_remaining: u32,
}

impl From<DisconnectOutcome> for DisconnectResponse {
    fn from(outcome: DisconnectOutcome) -> Self {
        Self {
            cancelled_deliveries: outcome.cancelled_deliveries,
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
            status: row.status.into(),
            violations: row.violations,
            field: row.field,
            account_ids: row.account_ids.into_iter().map(|a| a.as_uuid()).collect(),
            display_name_ignored: row.display_name_ignored,
        }
    }
}

/// The result of an import, in file order.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportReportDto {
    /// Whether accounts were connected (`true`) or the file was only checked.
    pub commit: bool,
    /// One entry per data row.
    pub rows: Vec<ImportRowDto>,
    /// How many rows ended each way, keyed by status.
    pub counts: BTreeMap<ImportStatusDto, u32>,
}

impl From<ImportReport> for ImportReportDto {
    fn from(report: ImportReport) -> Self {
        Self {
            commit: report.commit,
            rows: report.rows.into_iter().map(Into::into).collect(),
            counts: report
                .counts
                .into_iter()
                .map(|(status, n)| (status.into(), n))
                .collect(),
        }
    }
}
