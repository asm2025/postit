//! Credential grants, connected accounts, and their tokens.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::OwnerOnly`], so a delegate never
//! connects, disconnects, or re-authorizes an account. Account reads also apply the scope's
//! account limit.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AccountId, GrantId, PlatformId, RecurrenceId};
use postit_vault::{SealContext, Sealed};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::constraint::translate;
use crate::error::DataError;
use crate::paging;
use crate::text::{corrupt, parse, text_enum};
use crate::{Capability, OwnerScope};

fn platform(value: &str) -> Result<PlatformId, DataError> {
    PlatformId::new(value).map_err(corrupt)
}

text_enum! {
    /// How a grant was obtained.
    GrantKind {
        /// An OAuth authorization.
        Oauth => "oauth",
        /// Credentials the user typed in, such as a bot token or app password.
        Credentials => "credentials",
    }
}

text_enum! {
    /// Whether a grant can still be used.
    GrantStatus {
        /// Usable.
        Active => "active",
        /// The platform refused it; the owner must reconnect.
        NeedsReconnect => "needs_reconnect",
        /// The owner or the platform revoked it.
        Revoked => "revoked",
    }
}

text_enum! {
    /// Whether a connected account can be used.
    AccountStatus {
        /// Usable.
        Active => "active",
        /// Its grant needs the owner to reconnect.
        NeedsReconnect => "needs_reconnect",
        /// Its grant was revoked.
        Revoked => "revoked",
        /// The owner disconnected it; the row stays for history.
        Disconnected => "disconnected",
    }
}

impl From<GrantStatus> for AccountStatus {
    fn from(status: GrantStatus) -> Self {
        match status {
            GrantStatus::Active => Self::Active,
            GrantStatus::NeedsReconnect => Self::NeedsReconnect,
            GrantStatus::Revoked => Self::Revoked,
        }
    }
}

/// A stored credential grant. The credential stays sealed; this module never opens it.
#[derive(Debug, Clone)]
pub struct Grant {
    /// The grant.
    pub id: GrantId,
    /// The platform it authorizes.
    pub platform: PlatformId,
    /// How it was obtained.
    pub kind: GrantKind,
    /// The platform's ID for the authorizing user, if it has one.
    pub remote_user_id: Option<String>,
    /// The sealed credential. Seal it with [`GrantRepo::seal_context`].
    pub credential: Sealed<Value>,
    /// Non-secret connection settings, such as an instance URL.
    pub connection_config: Value,
    /// When the credential expires, if it does.
    pub expires_at: Option<DateTime<Utc>>,
    /// When the owner must reconnect, if the platform forces it.
    pub reconnect_by: Option<DateTime<Utc>>,
    /// The scopes the platform granted.
    pub scopes: Vec<String>,
    /// Whether it can still be used.
    pub status: GrantStatus,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A grant to store.
#[derive(Debug, Clone)]
pub struct NewGrant {
    /// The new grant's ID. The credential must be sealed with this ID in its context.
    pub id: GrantId,
    /// The platform it authorizes.
    pub platform: PlatformId,
    /// How it was obtained.
    pub kind: GrantKind,
    /// The platform's ID for the authorizing user, if it has one.
    pub remote_user_id: Option<String>,
    /// The sealed credential.
    pub credential: Sealed<Value>,
    /// Non-secret connection settings.
    pub connection_config: Value,
    /// When the credential expires, if it does.
    pub expires_at: Option<DateTime<Utc>>,
    /// When the owner must reconnect, if the platform forces it.
    pub reconnect_by: Option<DateTime<Utc>>,
    /// The scopes the platform granted.
    pub scopes: Vec<String>,
}

struct GrantRow {
    id: Uuid,
    platform_id: String,
    kind: String,
    remote_user_id: Option<String>,
    credential: Sealed<Value>,
    connection_config: Value,
    expires_at: Option<DateTime<Utc>>,
    reconnect_by: Option<DateTime<Utc>>,
    scopes: Vec<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<GrantRow> for Grant {
    type Error = DataError;

    fn try_from(row: GrantRow) -> Result<Self, DataError> {
        Ok(Self {
            id: GrantId::from(row.id),
            platform: platform(&row.platform_id)?,
            kind: parse(&row.kind)?,
            remote_user_id: row.remote_user_id,
            credential: row.credential,
            connection_config: row.connection_config,
            expires_at: row.expires_at,
            reconnect_by: row.reconnect_by,
            scopes: row.scopes,
            status: parse(&row.status)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Persistence for `account_grants`.
pub struct GrantRepo;

impl GrantRepo {
    /// Where a grant's credential is sealed: the `credential` column of its own row.
    #[must_use]
    pub fn seal_context(id: GrantId) -> SealContext {
        SealContext {
            table: "account_grants",
            column: "credential",
            row: id.as_uuid(),
        }
    }

    /// Stores a new grant for the scope's owner with status `active`.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn insert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewGrant,
    ) -> Result<Grant, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let row = sqlx::query_as!(
            GrantRow,
            r#"INSERT INTO account_grants
                   (id, owner_id, platform_id, kind, remote_user_id, credential, connection_config,
                    expires_at, reconnect_by, scopes, created_by, updated_by)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
               RETURNING id, platform_id, kind, remote_user_id,
                         credential AS "credential: Sealed<Value>",
                         connection_config, expires_at, reconnect_by, scopes, status,
                         created_at, updated_at"#,
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.platform.as_str(),
            new.kind.as_str(),
            new.remote_user_id,
            &new.credential as &Sealed<Value>,
            new.connection_config,
            new.expires_at,
            new.reconnect_by,
            &new.scopes,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(translate)?;
        row.try_into()
    }

    /// One grant of the scope's owner.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such grant.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: GrantId,
    ) -> Result<Grant, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            GrantRow,
            r#"SELECT id, platform_id, kind, remote_user_id,
                      credential AS "credential: Sealed<Value>",
                      connection_config, expires_at, reconnect_by, scopes, status,
                      created_at, updated_at
               FROM account_grants WHERE owner_id = $1 AND id = $2"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Sets the grant's status and the status of every account on it that is not
    /// `disconnected`, in one transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such grant.
    pub async fn set_status(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: GrantId,
        status: GrantStatus,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = conn.begin().await?;
        let updated = sqlx::query!(
            "UPDATE account_grants SET status = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            status.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        sqlx::query!(
            "UPDATE connected_accounts SET status = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND grant_id = $2 AND status <> 'disconnected'",
            scope.owner.as_uuid(),
            id.as_uuid(),
            AccountStatus::from(status).as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// How many accounts use the grant.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn account_count(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: GrantId,
    ) -> Result<i64, DataError> {
        scope.require(Capability::View)?;
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM connected_accounts WHERE owner_id = $1 AND grant_id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(count.unwrap_or(0))
    }

    /// Deletes the grant when no account uses it; returns whether it did.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn delete_if_unused(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: GrantId,
    ) -> Result<bool, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let deleted = sqlx::query!(
            "DELETE FROM account_grants g
             WHERE g.owner_id = $1 AND g.id = $2
               AND NOT EXISTS (SELECT 1 FROM connected_accounts a WHERE a.grant_id = g.id)",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(deleted > 0)
    }
}

/// A connected account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The account.
    pub id: AccountId,
    /// The grant that connects it; `None` once disconnected.
    pub grant_id: Option<GrantId>,
    /// The platform it lives on.
    pub platform: PlatformId,
    /// The platform's ID for it.
    pub remote_account_id: String,
    /// The name shown in postit.
    pub display_name: String,
    /// Its avatar, if the platform gives one.
    pub avatar_url: Option<String>,
    /// The scopes the platform granted.
    pub scopes: Vec<String>,
    /// Whether it can be used.
    pub status: AccountStatus,
    /// Whether it is a test account, so publishing to it needs no live confirmation.
    pub test_account: bool,
    /// When the connection was last verified.
    pub last_checked_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// An account to connect.
#[derive(Debug, Clone)]
pub struct NewAccount {
    /// The ID used when the account is new. An existing row of the owner keeps its own ID.
    pub id: AccountId,
    /// The grant that connects it.
    pub grant_id: GrantId,
    /// The platform it lives on.
    pub platform: PlatformId,
    /// The platform's ID for it.
    pub remote_account_id: String,
    /// The name shown in postit.
    pub display_name: String,
    /// Its avatar, if the platform gives one.
    pub avatar_url: Option<String>,
    /// The scopes the platform granted.
    pub scopes: Vec<String>,
    /// Whether it is a test account.
    pub test_account: bool,
}

/// What [`AccountRepo::insert_or_reactivate`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountUpsert {
    /// A new row.
    Created(Account),
    /// The owner's disconnected row, or an active one on the same grant, is active again.
    Reactivated(Account),
    /// The owner's row moved from another grant to the new one.
    Moved {
        /// The account on its new grant.
        account: Account,
        /// The grant it was on, which the caller may now find unused.
        previous_grant: GrantId,
    },
}

impl AccountUpsert {
    /// The account, whichever way it was stored.
    #[must_use]
    pub fn into_account(self) -> Account {
        match self {
            Self::Created(a) | Self::Reactivated(a) | Self::Moved { account: a, .. } => a,
        }
    }
}

/// Who has a remote account, as seen by one owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAccount {
    /// Nobody has it connected.
    Free,
    /// The owner has a row for it; `active` is false once they disconnected it.
    Own {
        /// The owner's row.
        account: AccountId,
        /// Whether the row is connected.
        active: bool,
    },
    /// Another owner has it connected.
    Elsewhere,
}

/// What [`AccountRepo::disconnect`] cleaned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disconnected {
    /// The account's grant now has no account, so the caller may delete it.
    pub grant_now_unused: bool,
    /// Recurrences paused because they were left with no target.
    pub paused_recurrences: Vec<RecurrenceId>,
}

struct AccountRow {
    id: Uuid,
    grant_id: Option<Uuid>,
    platform_id: String,
    remote_account_id: String,
    display_name: String,
    avatar_url: Option<String>,
    scopes: Vec<String>,
    status: String,
    test_account: bool,
    last_checked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<AccountRow> for Account {
    type Error = DataError;

    fn try_from(row: AccountRow) -> Result<Self, DataError> {
        Ok(Self {
            id: AccountId::from(row.id),
            grant_id: row.grant_id.map(GrantId::from),
            platform: platform(&row.platform_id)?,
            remote_account_id: row.remote_account_id,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            scopes: row.scopes,
            status: parse(&row.status)?,
            test_account: row.test_account,
            last_checked_at: row.last_checked_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Persistence for `connected_accounts`.
pub struct AccountRepo;

impl AccountRepo {
    /// Connects an account for the scope's owner.
    ///
    /// The owner's own row for the same remote account is reused: a disconnected one is
    /// reactivated, and an active one on another grant is moved to the new grant. Either way
    /// the row keeps the user's `display_name` and `test_account`; the avatar and scopes come
    /// from the platform. Otherwise a new row is made, which a remote account connected to
    /// another owner blocks.
    ///
    /// # Errors
    ///
    /// [`DataError::AccountConnectedElsewhere`] when another owner has the remote account
    /// connected, and [`DataError::SendingMethodExists`] when the owner already has an email
    /// sending method. Neither names the other owner.
    pub async fn insert_or_reactivate(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewAccount,
    ) -> Result<AccountUpsert, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = conn.begin().await?;
        let existing = sqlx::query!(
            "SELECT id, grant_id, status FROM connected_accounts
             WHERE owner_id = $1 AND platform_id = $2 AND remote_account_id = $3
             ORDER BY updated_at DESC LIMIT 1
             FOR UPDATE",
            scope.owner.as_uuid(),
            new.platform.as_str(),
            new.remote_account_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let upsert = if let Some(existing) = existing {
            let previous_grant = existing
                .grant_id
                .filter(|g| {
                    existing.status != AccountStatus::Disconnected.as_str()
                        && *g != new.grant_id.as_uuid()
                })
                .map(GrantId::from);
            let row = sqlx::query_as!(
                AccountRow,
                "UPDATE connected_accounts
                 SET grant_id = $3, avatar_url = $4, scopes = $5,
                     status = 'active', updated_at = now(), updated_by = $6
                 WHERE owner_id = $1 AND id = $2
                 RETURNING id, grant_id, platform_id, remote_account_id, display_name, avatar_url,
                           scopes, status, test_account, last_checked_at, created_at, updated_at",
                scope.owner.as_uuid(),
                existing.id,
                new.grant_id.as_uuid(),
                new.avatar_url,
                &new.scopes,
                scope.actor.as_uuid(),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(translate)?;
            let account = Account::try_from(row)?;
            match previous_grant {
                Some(previous_grant) => AccountUpsert::Moved {
                    account,
                    previous_grant,
                },
                None => AccountUpsert::Reactivated(account),
            }
        } else {
            let row = sqlx::query_as!(
                AccountRow,
                "INSERT INTO connected_accounts
                     (id, owner_id, grant_id, platform_id, remote_account_id, display_name,
                      avatar_url, scopes, test_account, created_by, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
                 RETURNING id, grant_id, platform_id, remote_account_id, display_name, avatar_url,
                           scopes, status, test_account, last_checked_at, created_at, updated_at",
                new.id.as_uuid(),
                scope.owner.as_uuid(),
                new.grant_id.as_uuid(),
                new.platform.as_str(),
                new.remote_account_id,
                new.display_name,
                new.avatar_url,
                &new.scopes,
                new.test_account,
                scope.actor.as_uuid(),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(translate)?;
            AccountUpsert::Created(row.try_into()?)
        };
        tx.commit().await?;
        Ok(upsert)
    }

    /// One account the scope reaches.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account or the scope's account limit
    /// excludes it.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountId,
    ) -> Result<Account, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        sqlx::query_as!(
            AccountRow,
            "SELECT id, grant_id, platform_id, remote_account_id, display_name, avatar_url,
                    scopes, status, test_account, last_checked_at, created_at, updated_at
             FROM connected_accounts
             WHERE owner_id = $1 AND id = $2 AND ($3::uuid[] IS NULL OR id = ANY($3))",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The accounts the scope reaches, newest first, optionally on one platform. Disconnected
    /// accounts are left out unless `include_disconnected`.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
        platform: Option<&PlatformId>,
        include_disconnected: bool,
    ) -> Result<ResultSet<Account>, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        let platform = platform.map(PlatformId::as_str);
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            AccountRow,
            "SELECT id, grant_id, platform_id, remote_account_id, display_name, avatar_url,
                    scopes, status, test_account, last_checked_at, created_at, updated_at
             FROM connected_accounts
             WHERE owner_id = $1 AND ($2::uuid[] IS NULL OR id = ANY($2))
               AND ($3::text IS NULL OR platform_id = $3)
               AND ($4 OR status <> 'disconnected')
             ORDER BY created_at DESC, id DESC
             LIMIT $5 OFFSET $6",
            scope.owner.as_uuid(),
            filter.as_deref(),
            platform,
            include_disconnected,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM connected_accounts
             WHERE owner_id = $1 AND ($2::uuid[] IS NULL OR id = ANY($2))
               AND ($3::text IS NULL OR platform_id = $3)
               AND ($4 OR status <> 'disconnected')",
            scope.owner.as_uuid(),
            filter.as_deref(),
            platform,
            include_disconnected,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(Account::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Changes the user's settings on an account: the test flag and the display name. A `None`
    /// leaves that setting as it is.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account or it is disconnected.
    pub async fn update_settings(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountId,
        test_account: Option<bool>,
        display_name: Option<String>,
    ) -> Result<Account, DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query_as!(
            AccountRow,
            "UPDATE connected_accounts
             SET test_account = COALESCE($3, test_account),
                 display_name = COALESCE($4, display_name),
                 updated_at = now(), updated_by = $5
             WHERE owner_id = $1 AND id = $2 AND status <> 'disconnected'
             RETURNING id, grant_id, platform_id, remote_account_id, display_name, avatar_url,
                       scopes, status, test_account, last_checked_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            test_account,
            display_name,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The IDs of the accounts connected through `grant` that are not disconnected.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn ids_on_grant(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        grant: GrantId,
    ) -> Result<Vec<AccountId>, DataError> {
        scope.require(Capability::View)?;
        let ids = sqlx::query_scalar!(
            "SELECT id FROM connected_accounts
             WHERE owner_id = $1 AND grant_id = $2 AND status <> 'disconnected'
             ORDER BY created_at, id",
            scope.owner.as_uuid(),
            grant.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(ids.into_iter().map(AccountId::from).collect())
    }

    /// Who has a remote account: the scope's owner, someone else, or nobody. Only says
    /// "elsewhere", never who. Used to classify import rows before anything is connected.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn remote_account(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        platform: &PlatformId,
        remote_account_id: &str,
    ) -> Result<RemoteAccount, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let own = sqlx::query!(
            "SELECT id, status FROM connected_accounts
             WHERE owner_id = $1 AND platform_id = $2 AND remote_account_id = $3
             ORDER BY updated_at DESC LIMIT 1",
            scope.owner.as_uuid(),
            platform.as_str(),
            remote_account_id,
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(own) = own {
            return Ok(RemoteAccount::Own {
                account: AccountId::from(own.id),
                active: own.status != AccountStatus::Disconnected.as_str(),
            });
        }
        let elsewhere = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM connected_accounts
                              WHERE owner_id <> $1 AND platform_id = $2
                                AND remote_account_id = $3 AND status <> 'disconnected')
               AS "found!""#,
            scope.owner.as_uuid(),
            platform.as_str(),
            remote_account_id,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(if elsewhere {
            RemoteAccount::Elsewhere
        } else {
            RemoteAccount::Free
        })
    }

    /// Sets one account's status. Use [`AccountRepo::disconnect`] to disconnect.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account, and
    /// [`DataError::Conflict`] for `disconnected`.
    pub async fn set_status(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountId,
        status: AccountStatus,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        if status == AccountStatus::Disconnected {
            return Err(DataError::Conflict("use_disconnect".to_string()));
        }
        let updated = sqlx::query!(
            "UPDATE connected_accounts SET status = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2 AND status <> 'disconnected'",
            scope.owner.as_uuid(),
            id.as_uuid(),
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

    /// Records when the connection was last verified.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account.
    pub async fn mark_checked(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let updated = sqlx::query!(
            "UPDATE connected_accounts SET last_checked_at = $3
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Disconnects an account in one transaction: it becomes `disconnected` with no grant, its
    /// tokens, post overrides and targets, inbox items, target-group memberships, and
    /// recurrence targets are deleted, and recurrences left with no target are paused with
    /// reason `no_targets`.
    ///
    /// Pending and parked deliveries for the account are cancelled by the publishing
    /// repository's `cancel_for_account`; the service calls both in one transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account.
    pub async fn disconnect(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AccountId,
    ) -> Result<Disconnected, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let owner = scope.owner.as_uuid();
        let account = id.as_uuid();
        let mut tx = conn.begin().await?;

        let current = sqlx::query!(
            "SELECT grant_id FROM connected_accounts WHERE owner_id = $1 AND id = $2 FOR UPDATE",
            owner,
            account,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;

        sqlx::query!(
            "UPDATE connected_accounts
             SET status = 'disconnected', grant_id = NULL, inbox_cursor = NULL,
                 inbox_next_poll_at = NULL, updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2",
            owner,
            account,
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM account_tokens WHERE owner_id = $1 AND account_id = $2",
            owner,
            account
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM post_overrides WHERE owner_id = $1 AND account_id = $2",
            owner,
            account
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM post_targets WHERE owner_id = $1 AND account_id = $2",
            owner,
            account
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM inbox_items WHERE owner_id = $1 AND account_id = $2",
            owner,
            account
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM target_group_accounts WHERE owner_id = $1 AND account_id = $2",
            owner,
            account
        )
        .execute(&mut *tx)
        .await?;
        let affected = sqlx::query_scalar!(
            "DELETE FROM recurrence_targets WHERE owner_id = $1 AND account_id = $2
             RETURNING recurrence_id",
            owner,
            account
        )
        .fetch_all(&mut *tx)
        .await?;
        let paused = sqlx::query_scalar!(
            "UPDATE recurrences r
             SET paused = true, paused_reason = 'no_targets', updated_at = now(), updated_by = $3
             WHERE r.owner_id = $1 AND r.id = ANY($2) AND NOT r.paused
               AND NOT EXISTS (SELECT 1 FROM recurrence_targets t WHERE t.recurrence_id = r.id)
             RETURNING r.id",
            owner,
            &affected,
            scope.actor.as_uuid(),
        )
        .fetch_all(&mut *tx)
        .await?;

        let grant_now_unused = match current.grant_id {
            Some(grant) => sqlx::query_scalar!(
                "SELECT NOT EXISTS (SELECT 1 FROM connected_accounts
                                        WHERE owner_id = $1 AND grant_id = $2)",
                owner,
                grant,
            )
            .fetch_one(&mut *tx)
            .await?
            .unwrap_or(false),
            None => false,
        };
        tx.commit().await?;
        Ok(Disconnected {
            grant_now_unused,
            paused_recurrences: paused.into_iter().map(RecurrenceId::from).collect(),
        })
    }
}

/// Persistence for `account_tokens`, the sealed access token of an OAuth account.
///
/// The row ID is the account ID, so a token is sealed with [`AccountTokenRepo::seal_context`].
pub struct AccountTokenRepo;

impl AccountTokenRepo {
    /// Where an account's token is sealed: the `access_token` column of its own row.
    #[must_use]
    pub fn seal_context(account: AccountId) -> SealContext {
        SealContext {
            table: "account_tokens",
            column: "access_token",
            row: account.as_uuid(),
        }
    }

    /// Stores or replaces the token of an account of the scope's owner.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such account.
    pub async fn upsert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        account: AccountId,
        token: &Sealed<Value>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let stored = sqlx::query!(
            "INSERT INTO account_tokens (id, owner_id, account_id, access_token, expires_at,
                                         created_by, updated_by)
             SELECT a.id, a.owner_id, a.id, $3, $4, $5, $5
             FROM connected_accounts a WHERE a.owner_id = $1 AND a.id = $2
             ON CONFLICT (account_id) DO UPDATE
             SET access_token = EXCLUDED.access_token, expires_at = EXCLUDED.expires_at,
                 updated_at = now(), updated_by = EXCLUDED.updated_by",
            scope.owner.as_uuid(),
            account.as_uuid(),
            token as &Sealed<Value>,
            expires_at,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if stored == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// The sealed token of an account the scope reaches, if it has one.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        account: AccountId,
    ) -> Result<Option<(Sealed<Value>, Option<DateTime<Utc>>)>, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        let row = sqlx::query!(
            r#"SELECT access_token AS "access_token: Sealed<Value>", expires_at
               FROM account_tokens
               WHERE owner_id = $1 AND account_id = $2
                 AND ($3::uuid[] IS NULL OR account_id = ANY($3))"#,
            scope.owner.as_uuid(),
            account.as_uuid(),
            filter.as_deref(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(|r| (r.access_token, r.expires_at)))
    }
}
