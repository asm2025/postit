//! Wire types of the delegation, workspace, and user lookup routes. None carries an email:
//! a user appears by ID, display name, and status only.

use chrono::{DateTime, Utc};
use postit_data::AccountLimit;
use postit_data::Level;
use postit_data::delegations::{Delegation, EndedReason};
use postit_data::users::UserStatus;
use postit_identity::delegation::Workspace;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::StatusDto;

/// What a delegate may do in the owner's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LevelDto {
    /// Read.
    View,
    /// Read and change.
    Edit,
    /// Read, change, and publish.
    Publish,
}

impl From<Level> for LevelDto {
    fn from(level: Level) -> Self {
        match level {
            Level::View => Self::View,
            Level::Edit => Self::Edit,
            Level::Publish => Self::Publish,
        }
    }
}

impl From<LevelDto> for Level {
    fn from(level: LevelDto) -> Self {
        match level {
            LevelDto::View => Self::View,
            LevelDto::Edit => Self::Edit,
            LevelDto::Publish => Self::Publish,
        }
    }
}

/// Why a delegation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndedReasonDto {
    /// The owner revoked it.
    Revoked,
    /// The delegate left it.
    Left,
    /// It reached its expiry.
    Expired,
}

impl From<EndedReason> for EndedReasonDto {
    fn from(reason: EndedReason) -> Self {
        match reason {
            EndedReason::Revoked => Self::Revoked,
            EndedReason::Left => Self::Left,
            EndedReason::Expired => Self::Expired,
        }
    }
}

/// One side of a delegation: a user by ID, current name, and status.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DelegationUserDto {
    /// User ID.
    pub id: Uuid,
    /// The user's current name.
    pub display_name: String,
    /// The user's account status. A grant only works while both users are `active`.
    pub status: StatusDto,
}

impl DelegationUserDto {
    /// The user `id` named `display_name` with `status`.
    #[must_use]
    pub fn new(id: postit_core::UserId, display_name: String, status: UserStatus) -> Self {
        Self {
            id: id.as_uuid(),
            display_name,
            status: status.into(),
        }
    }
}

/// A delegation: the owner lets the delegate act in the owner's workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct DelegationDto {
    /// Delegation ID.
    pub id: Uuid,
    /// The user whose workspace it opens.
    pub owner: DelegationUserDto,
    /// The user it lets in.
    pub delegate: DelegationUserDto,
    /// What the delegate may do.
    pub level: LevelDto,
    /// Whether the delegate reaches every account; when false, only `account_ids`.
    pub all_accounts: bool,
    /// The accounts the delegate reaches when `all_accounts` is false (possibly none: posts
    /// and media only). Empty when `all_accounts` is true.
    pub account_ids: Vec<Uuid>,
    /// When it stops working; null for never.
    pub expires_at: Option<DateTime<Utc>>,
    /// When it ended; null while live.
    pub ended_at: Option<DateTime<Utc>>,
    /// Why it ended; null while live.
    pub ended_reason: Option<EndedReasonDto>,
    /// When it was granted.
    pub created_at: DateTime<Utc>,
    /// Who granted it.
    pub created_by: Option<Uuid>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// Who last changed it.
    pub updated_by: Option<Uuid>,
}

/// `(all_accounts, account_ids)` for the wire.
fn limit_parts(limit: &AccountLimit) -> (bool, Vec<Uuid>) {
    match limit {
        AccountLimit::All => (true, Vec::new()),
        AccountLimit::Only(ids) => (
            false,
            ids.iter().map(postit_core::AccountId::as_uuid).collect(),
        ),
    }
}

impl DelegationDto {
    /// `delegation` between `owner` and `delegate`.
    #[must_use]
    pub fn new(
        delegation: &Delegation,
        owner: DelegationUserDto,
        delegate: DelegationUserDto,
    ) -> Self {
        let (all_accounts, account_ids) = limit_parts(&delegation.accounts);
        Self {
            id: delegation.id.as_uuid(),
            owner,
            delegate,
            level: delegation.level.into(),
            all_accounts,
            account_ids,
            expires_at: delegation.expires_at,
            ended_at: delegation.ended_at,
            ended_reason: delegation.ended_reason.map(Into::into),
            created_at: delegation.created_at,
            created_by: delegation.created_by.map(|u| u.as_uuid()),
            updated_at: delegation.updated_at,
            updated_by: delegation.updated_by.map(|u| u.as_uuid()),
        }
    }
}

/// A new delegation in the caller's workspace.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantDelegationRequest {
    /// The user to let in: an `active` user other than the caller (see `/users/lookup`).
    pub delegate_id: Uuid,
    /// What they may do.
    pub level: LevelDto,
    /// Limit the grant to these of the caller's connected accounts; an empty list means posts
    /// and media only. Absent means every account.
    pub account_ids: Option<Vec<Uuid>>,
    /// When it stops working, in the future; absent or null for never.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Wraps a present value, `null` included, in `Some`, so a missing field (`#[serde(default)]`)
/// stays `None` and `null` becomes `Some(None)`.
#[allow(
    clippy::option_option,
    reason = "absent, null, and a value are three different requests"
)]
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Changes to a live delegation. Absent fields stay as they are.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeDelegationRequest {
    /// A new level.
    pub level: Option<LevelDto>,
    /// `true` lifts the account limit. `false` is refused: send `account_ids` instead.
    /// Cannot be sent together with `account_ids`.
    pub all_accounts: Option<bool>,
    /// A new account limit, replacing the old one; an empty list means posts and media only.
    pub account_ids: Option<Vec<Uuid>>,
    /// A new expiry in the future, or `null` to clear it. Absent leaves it unchanged, so a
    /// client that always serializes nulls must send the current value to keep it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<DateTime<Utc>>)]
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// A workspace the caller can act in: their own, or one a live grant opens.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceDto {
    /// The workspace's owner. Send their ID in `X-Postit-Act-As` to act here (not for the
    /// caller's own workspace).
    pub owner: DelegationUserDto,
    /// Whether this is the caller's own workspace.
    pub own: bool,
    /// The grant that opens it; null for the caller's own.
    pub delegation_id: Option<Uuid>,
    /// What the caller may do here; null for the caller's own (everything).
    pub level: Option<LevelDto>,
    /// Whether the caller reaches every account here; null for the caller's own.
    pub all_accounts: Option<bool>,
    /// The accounts the caller reaches when `all_accounts` is false; null for the caller's
    /// own.
    pub account_ids: Option<Vec<Uuid>>,
}

impl From<Workspace> for WorkspaceDto {
    fn from(workspace: Workspace) -> Self {
        // Workspaces are listed only for an active caller and active owners.
        let owner = DelegationUserDto::new(
            workspace.owner_id,
            workspace.owner_display_name,
            UserStatus::Active,
        );
        match workspace.access {
            None => Self {
                owner,
                own: true,
                delegation_id: None,
                level: None,
                all_accounts: None,
                account_ids: None,
            },
            Some((id, level, limit)) => {
                let (all_accounts, account_ids) = limit_parts(&limit);
                Self {
                    owner,
                    own: false,
                    delegation_id: Some(id.as_uuid()),
                    level: Some(level.into()),
                    all_accounts: Some(all_accounts),
                    account_ids: Some(account_ids),
                }
            }
        }
    }
}

/// A user found by email.
#[derive(Debug, Serialize, ToSchema)]
pub struct LookupUserDto {
    /// User ID; pass it as `delegate_id`.
    pub id: Uuid,
    /// The user's current name.
    pub display_name: String,
}

/// The result of a user lookup.
#[derive(Debug, Serialize, ToSchema)]
pub struct LookupResponse {
    /// The one matching user, or null.
    pub user: Option<LookupUserDto>,
}

#[cfg(test)]
mod tests {
    use super::ChangeDelegationRequest;

    #[test]
    fn expires_at_is_tri_state() {
        let parse = |text: &str| {
            serde_json::from_str::<ChangeDelegationRequest>(text)
                .unwrap_or_else(|e| unreachable!("{e}"))
                .expires_at
        };
        assert_eq!(parse("{}"), None);
        assert_eq!(parse(r#"{"expires_at":null}"#), Some(None));
        assert!(matches!(
            parse(r#"{"expires_at":"2030-01-01T00:00:00Z"}"#),
            Some(Some(_))
        ));
    }
}
