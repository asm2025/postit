//! Delegation grants: an owner lets another user act in their workspace at a level, on every
//! account or a limited set.
//!
//! Owner-side methods take the caller's [`OwnerScope`] and require
//! [`Capability::OwnerOnly`], so a delegate can never grant, change, or list grants (no
//! re-delegation). The delegate-side methods ([`DelegationRepo::lock_received`],
//! [`DelegationRepo::list_received`]) and [`DelegationRepo::end`] take plain IDs; the service
//! decides who may call them. Every timestamp that decides liveness comes from the caller's
//! clock, never the database's `now()`.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AccountId, DelegationId, UserId};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::constraint::translate;
use crate::error::DataError;
use crate::paging;
use crate::text::{parse, text_enum};
use crate::users::UserStatus;
use crate::{AccountLimit, Capability, Level, OwnerScope};

text_enum! {
    /// Why a grant ended.
    EndedReason {
        /// The owner revoked it.
        Revoked => "revoked",
        /// The delegate left it.
        Left => "left",
        /// It reached its expiry.
        Expired => "expired",
    }
}

/// A delegation grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// The grant.
    pub id: DelegationId,
    /// The user whose workspace it opens.
    pub owner_id: UserId,
    /// The user it lets in.
    pub delegate_id: UserId,
    /// What the delegate may do.
    pub level: Level,
    /// Which accounts the delegate reaches.
    pub accounts: AccountLimit,
    /// When it stops working; `None` for never.
    pub expires_at: Option<DateTime<Utc>>,
    /// When it was ended; `None` while live.
    pub ended_at: Option<DateTime<Utc>>,
    /// Why it was ended; set exactly when `ended_at` is.
    pub ended_reason: Option<EndedReason>,
    /// The user who revoked it, for [`EndedReason::Revoked`].
    pub revoked_by: Option<UserId>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// Who created it.
    pub created_by: Option<UserId>,
    /// Who last changed it.
    pub updated_by: Option<UserId>,
}

impl Delegation {
    /// Whether the grant works at `now`: not ended, and not past its expiry.
    #[must_use]
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.ended_at.is_none() && self.expires_at.is_none_or(|at| at > now)
    }
}

/// A grant with the other party's name and status: the delegate on the owner's list, the owner
/// on the delegate's received list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationView {
    /// The grant.
    pub delegation: Delegation,
    /// The other party's display name.
    pub other_display_name: String,
    /// The other party's status.
    pub other_status: UserStatus,
}

/// A grant to create in the scope owner's workspace.
#[derive(Debug, Clone)]
pub struct NewDelegation {
    /// The new grant's ID.
    pub id: DelegationId,
    /// The user to let in.
    pub delegate_id: UserId,
    /// What they may do.
    pub level: Level,
    /// Which accounts they reach.
    pub accounts: AccountLimit,
    /// When it stops working; `None` for never.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Changes to a grant. `None` leaves a field as it is; `expires_at: Some(None)` clears the
/// expiry.
#[derive(Debug, Clone, Default)]
pub struct DelegationChange {
    /// A new level.
    pub level: Option<Level>,
    /// A new account limit, replacing the old one.
    pub accounts: Option<AccountLimit>,
    /// A new expiry, or `Some(None)` for none.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

struct DelegationRow {
    id: Uuid,
    owner_id: Uuid,
    delegate_id: Uuid,
    level: String,
    all_accounts: bool,
    account_ids: Vec<Uuid>,
    expires_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    ended_reason: Option<String>,
    revoked_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl TryFrom<DelegationRow> for Delegation {
    type Error = DataError;

    fn try_from(row: DelegationRow) -> Result<Self, DataError> {
        Ok(Self {
            id: DelegationId::from(row.id),
            owner_id: UserId::from(row.owner_id),
            delegate_id: UserId::from(row.delegate_id),
            level: parse(&row.level)?,
            accounts: if row.all_accounts {
                AccountLimit::All
            } else {
                AccountLimit::only(row.account_ids.into_iter().map(AccountId::from))
            },
            expires_at: row.expires_at,
            ended_at: row.ended_at,
            ended_reason: row.ended_reason.as_deref().map(parse).transpose()?,
            revoked_by: row.revoked_by.map(UserId::from),
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by: row.created_by.map(UserId::from),
            updated_by: row.updated_by.map(UserId::from),
        })
    }
}

struct ViewRow {
    id: Uuid,
    owner_id: Uuid,
    delegate_id: Uuid,
    level: String,
    all_accounts: bool,
    account_ids: Vec<Uuid>,
    expires_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    ended_reason: Option<String>,
    revoked_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    other_display_name: String,
    other_status: String,
}

impl TryFrom<ViewRow> for DelegationView {
    type Error = DataError;

    fn try_from(row: ViewRow) -> Result<Self, DataError> {
        let other_status = parse(&row.other_status)?;
        Ok(Self {
            delegation: DelegationRow {
                id: row.id,
                owner_id: row.owner_id,
                delegate_id: row.delegate_id,
                level: row.level,
                all_accounts: row.all_accounts,
                account_ids: row.account_ids,
                expires_at: row.expires_at,
                ended_at: row.ended_at,
                ended_reason: row.ended_reason,
                revoked_by: row.revoked_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by: row.created_by,
                updated_by: row.updated_by,
            }
            .try_into()?,
            other_display_name: row.other_display_name,
            other_status,
        })
    }
}

/// The account IDs a limit names, sorted and deduplicated; `None` for every account.
fn limit_ids(accounts: &AccountLimit) -> Option<Vec<Uuid>> {
    match accounts {
        AccountLimit::All => None,
        AccountLimit::Only(ids) => {
            let mut ids: Vec<Uuid> = ids.iter().map(AccountId::as_uuid).collect();
            ids.sort_unstable();
            ids.dedup();
            Some(ids)
        }
    }
}

/// Persistence for `delegations` and `delegation_accounts`.
pub struct DelegationRepo;

impl DelegationRepo {
    /// Creates a grant in the scope owner's workspace and, for a limited grant, its account
    /// rows, in one transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] for a delegated scope; [`DataError::Conflict`] with
    /// `account_unavailable` when a listed account is not the owner's or is disconnected,
    /// `delegation_exists` when the pair already has a live grant, and `self_delegation` when
    /// the delegate is the owner.
    pub async fn insert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewDelegation,
    ) -> Result<Delegation, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let ids = limit_ids(&new.accounts);
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "INSERT INTO delegations
                 (id, owner_id, delegate_id, level, all_accounts, expires_at, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7)",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.delegate_id.as_uuid(),
            new.level.as_str(),
            ids.is_none(),
            new.expires_at,
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await
        .map_err(translate)?;
        if let Some(ids) = &ids {
            Self::lock_accounts(&mut tx, scope.owner, ids).await?;
            Self::insert_links(&mut tx, scope.owner, new.id, ids).await?;
        }
        let delegation = Self::fetch(&mut tx, scope.owner, new.id).await?;
        tx.commit().await?;
        Ok(delegation)
    }

    /// Locks the account rows `ids` of `owner` `FOR SHARE` and checks each is connected. The
    /// lock waits for a concurrent `AccountRepo::disconnect`, which locks the row `FOR UPDATE`,
    /// and then sees its result, so a grant never names an account disconnected before the
    /// grant commits. Callers take this lock before touching any `delegation_accounts` row,
    /// the same order as `disconnect` (account row first, then link rows), so the two never
    /// deadlock.
    async fn lock_accounts(
        conn: &mut PgConnection,
        owner: UserId,
        ids: &[Uuid],
    ) -> Result<(), DataError> {
        let available = sqlx::query_scalar!(
            "SELECT id FROM connected_accounts
             WHERE owner_id = $1 AND id = ANY($2) AND status <> 'disconnected'
             ORDER BY id
             FOR SHARE",
            owner.as_uuid(),
            ids,
        )
        .fetch_all(&mut *conn)
        .await?;
        if available.len() != ids.len() {
            return Err(DataError::Conflict("account_unavailable".to_string()));
        }
        Ok(())
    }

    /// Records `ids` as the grant's limit. Call [`DelegationRepo::lock_accounts`] first.
    async fn insert_links(
        conn: &mut PgConnection,
        owner: UserId,
        delegation: DelegationId,
        ids: &[Uuid],
    ) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO delegation_accounts (owner_id, delegation_id, account_id)
             SELECT $1, $2, unnest($3::uuid[])",
            owner.as_uuid(),
            delegation.as_uuid(),
            ids,
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?;
        Ok(())
    }

    /// The owner's grant `id`, ended or not.
    async fn fetch(
        conn: &mut PgConnection,
        owner: UserId,
        id: DelegationId,
    ) -> Result<Delegation, DataError> {
        sqlx::query_as!(
            DelegationRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by
               FROM delegations d
               WHERE d.owner_id = $1 AND d.id = $2"#,
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Locks the pair's grant that has not been ended (expired or not), if there is one.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] for a delegated scope.
    pub async fn lock_live_for_pair(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        delegate: UserId,
    ) -> Result<Option<Delegation>, DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query_as!(
            DelegationRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by
               FROM delegations d
               WHERE d.owner_id = $1 AND d.delegate_id = $2 AND d.ended_at IS NULL
               FOR UPDATE"#,
            scope.owner.as_uuid(),
            delegate.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Delegation::try_from)
        .transpose()
    }

    /// Locks one of the owner's grants, ended or not.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] for a delegated scope, and [`DataError::NotFound`] when the owner
    /// has no such grant.
    pub async fn lock(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DelegationId,
    ) -> Result<Delegation, DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query_as!(
            DelegationRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by
               FROM delegations d
               WHERE d.owner_id = $1 AND d.id = $2
               FOR UPDATE"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Locks a grant `delegate` holds, ended or not, for the leave path.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when `delegate` holds no such grant.
    pub async fn lock_received(
        conn: &mut PgConnection,
        delegate: UserId,
        id: DelegationId,
    ) -> Result<Delegation, DataError> {
        sqlx::query_as!(
            DelegationRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by
               FROM delegations d
               WHERE d.delegate_id = $1 AND d.id = $2
               FOR UPDATE"#,
            delegate.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Applies the present fields of `change` to one of the owner's grants that has not been
    /// ended. A new account limit replaces the old one, with the same checks as
    /// [`DelegationRepo::insert`].
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] for a delegated scope, [`DataError::NotFound`] when the owner has
    /// no such grant or it has ended, and [`DataError::Conflict`] with `account_unavailable`
    /// when a listed account is not the owner's or is disconnected.
    pub async fn update(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DelegationId,
        change: DelegationChange,
    ) -> Result<Delegation, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let all_accounts = change.accounts.as_ref().map(|a| *a == AccountLimit::All);
        let mut tx = conn.begin().await?;
        let updated = sqlx::query!(
            "UPDATE delegations
             SET level = COALESCE($3, level),
                 all_accounts = COALESCE($4, all_accounts),
                 expires_at = CASE WHEN $5 THEN $6 ELSE expires_at END,
                 updated_at = now(), updated_by = $7
             WHERE owner_id = $1 AND id = $2 AND ended_at IS NULL",
            scope.owner.as_uuid(),
            id.as_uuid(),
            change.level.map(Level::as_str),
            all_accounts,
            change.expires_at.is_some(),
            change.expires_at.flatten(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await
        .map_err(translate)?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        if let Some(accounts) = &change.accounts {
            // Account rows before link rows, as in `disconnect`: see `lock_accounts`.
            let ids = limit_ids(accounts);
            if let Some(ids) = &ids {
                Self::lock_accounts(&mut tx, scope.owner, ids).await?;
            }
            sqlx::query!(
                "DELETE FROM delegation_accounts WHERE owner_id = $1 AND delegation_id = $2",
                scope.owner.as_uuid(),
                id.as_uuid(),
            )
            .execute(&mut *tx)
            .await?;
            if let Some(ids) = &ids {
                Self::insert_links(&mut tx, scope.owner, id, ids).await?;
            }
        }
        let delegation = Self::fetch(&mut tx, scope.owner, id).await?;
        tx.commit().await?;
        Ok(delegation)
    }

    /// Ends a grant at `now` for `reason`, recording `revoked_by` for a revocation. An ended
    /// or missing grant is left as it is.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn end(
        conn: &mut PgConnection,
        id: DelegationId,
        reason: EndedReason,
        revoked_by: Option<UserId>,
        now: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE delegations
             SET ended_at = $2, ended_reason = $3, revoked_by = $4, updated_at = $2
             WHERE id = $1 AND ended_at IS NULL",
            id.as_uuid(),
            now,
            reason.as_str(),
            revoked_by.map(|u| u.as_uuid()),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// The owner's grants with each delegate's name and status, newest first. Ended grants
    /// are left out unless `include_ended`.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] for a delegated scope.
    pub async fn list_granted(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        include_ended: bool,
        pagination: Pagination,
    ) -> Result<ResultSet<DelegationView>, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            ViewRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by,
                      u.display_name AS other_display_name, u.status AS other_status
               FROM delegations d
               JOIN users u ON u.id = d.delegate_id
               WHERE d.owner_id = $1 AND ($2 OR d.ended_at IS NULL)
               ORDER BY d.created_at DESC, d.id DESC
               LIMIT $3 OFFSET $4"#,
            scope.owner.as_uuid(),
            include_ended,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM delegations
             WHERE owner_id = $1 AND ($2 OR ended_at IS NULL)",
            scope.owner.as_uuid(),
            include_ended,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(DelegationView::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// The grants `delegate` holds that have not been ended, with each owner's name and
    /// status, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list_received(
        conn: &mut PgConnection,
        delegate: UserId,
        pagination: Pagination,
    ) -> Result<ResultSet<DelegationView>, DataError> {
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            ViewRow,
            r#"SELECT d.id, d.owner_id, d.delegate_id, d.level, d.all_accounts,
                      array(SELECT a.account_id FROM delegation_accounts a
                            WHERE a.delegation_id = d.id ORDER BY a.account_id) AS "account_ids!",
                      d.expires_at, d.ended_at, d.ended_reason, d.revoked_by,
                      d.created_at, d.updated_at, d.created_by, d.updated_by,
                      u.display_name AS other_display_name, u.status AS other_status
               FROM delegations d
               JOIN users u ON u.id = d.owner_id
               WHERE d.delegate_id = $1 AND d.ended_at IS NULL
               ORDER BY d.created_at DESC, d.id DESC
               LIMIT $2 OFFSET $3"#,
            delegate.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM delegations WHERE delegate_id = $1 AND ended_at IS NULL",
            delegate.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(DelegationView::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }
}
