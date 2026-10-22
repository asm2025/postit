//! Target groups: named sets of connected accounts a post can be sent to in one pick.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. A group itself is visible to every
//! caller of the owner, but its members are filtered by the scope's account limit. A limited
//! delegate's writes touch only the members inside the limit and keep the hidden ones.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AccountId, TargetGroupId};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::error::DataError;
use crate::paging;
use crate::{Capability, OwnerScope};

/// A target group with the members the caller's limit reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetGroup {
    /// The group.
    pub id: TargetGroupId,
    /// Its name.
    pub name: String,
    /// Its member accounts inside the caller's limit, oldest member first.
    pub accounts: Vec<AccountId>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct GroupRow {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl GroupRow {
    fn with(self, accounts: Vec<AccountId>) -> TargetGroup {
        TargetGroup {
            id: TargetGroupId::from_uuid(self.id),
            name: self.name,
            accounts,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Target groups.
pub struct TargetGroupRepo;

impl TargetGroupRepo {
    /// Creates a group with its first members.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when an account is outside the caller's limit or not the
    /// owner's.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: TargetGroupId,
        name: &str,
        accounts: &[AccountId],
    ) -> Result<TargetGroup, DataError> {
        scope.require(Capability::Edit)?;
        let members = reachable(scope, accounts)?;
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "INSERT INTO target_groups (id, owner_id, name, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $4)",
            id.as_uuid(),
            scope.owner.as_uuid(),
            name,
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        add_members(&mut tx, scope, id, &members).await?;
        tx.commit().await?;
        Self::get(conn, scope, id).await
    }

    /// One group.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such group.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: TargetGroupId,
    ) -> Result<TargetGroup, DataError> {
        scope.require(Capability::View)?;
        let row = sqlx::query_as!(
            GroupRow,
            "SELECT id, name, created_at, updated_at FROM target_groups
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        let mut members = members_of(conn, scope, &[row.id]).await?;
        let accounts = members.remove(&row.id).unwrap_or_default();
        Ok(row.with(accounts))
    }

    /// The owner's groups by name.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<TargetGroup>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            GroupRow,
            "SELECT id, name, created_at, updated_at FROM target_groups
             WHERE owner_id = $1
             ORDER BY name, id
             LIMIT $2 OFFSET $3",
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM target_groups WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        let mut members = members_of(conn, scope, &ids).await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(|row| {
                    let accounts = members.remove(&row.id).unwrap_or_default();
                    row.with(accounts)
                })
                .collect(),
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Renames a group.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such group.
    pub async fn rename(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: TargetGroupId,
        name: &str,
    ) -> Result<TargetGroup, DataError> {
        scope.require(Capability::Edit)?;
        let renamed = sqlx::query!(
            "UPDATE target_groups SET name = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            name,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if renamed == 0 {
            return Err(DataError::NotFound);
        }
        Self::get(conn, scope, id).await
    }

    /// Replaces the members the caller's limit reaches. Members outside the limit stay.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such group, or an account is outside the
    /// caller's limit or not the owner's.
    pub async fn set_accounts(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: TargetGroupId,
        accounts: &[AccountId],
    ) -> Result<TargetGroup, DataError> {
        scope.require(Capability::Edit)?;
        let members = reachable(scope, accounts)?;
        let filter = scope.account_filter();
        let mut tx = conn.begin().await?;
        let touched = sqlx::query!(
            "UPDATE target_groups SET updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if touched == 0 {
            return Err(DataError::NotFound);
        }
        sqlx::query!(
            "DELETE FROM target_group_accounts
             WHERE owner_id = $1 AND group_id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .execute(&mut *tx)
        .await?;
        add_members(&mut tx, scope, id, &members).await?;
        tx.commit().await?;
        Self::get(conn, scope, id).await
    }

    /// Deletes a group. Its accounts and the posts sent through it are untouched.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such group.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: TargetGroupId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let deleted = sqlx::query!(
            "DELETE FROM target_groups WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if deleted == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }
}

/// `accounts` as distinct UUIDs, or `NotFound` when one is outside the scope's limit.
fn reachable(scope: &OwnerScope, accounts: &[AccountId]) -> Result<Vec<Uuid>, DataError> {
    if accounts
        .iter()
        .any(|account| !scope.allows_account(*account))
    {
        return Err(DataError::NotFound);
    }
    let mut ids: Vec<Uuid> = accounts.iter().map(AccountId::as_uuid).collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

async fn add_members(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    group: TargetGroupId,
    accounts: &[Uuid],
) -> Result<(), DataError> {
    for account in accounts {
        let inserted = sqlx::query!(
            "INSERT INTO target_group_accounts (group_id, account_id, owner_id)
             SELECT $2, id, owner_id FROM connected_accounts
             WHERE owner_id = $1 AND id = $3",
            scope.owner.as_uuid(),
            group.as_uuid(),
            account,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Err(DataError::NotFound);
        }
    }
    Ok(())
}

/// The members of `groups` the scope's limit reaches, keyed by group.
async fn members_of(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    groups: &[Uuid],
) -> Result<HashMap<Uuid, Vec<AccountId>>, DataError> {
    let filter = scope.account_filter();
    let rows = sqlx::query!(
        "SELECT group_id, account_id FROM target_group_accounts
         WHERE owner_id = $1 AND group_id = ANY($2)
           AND ($3::uuid[] IS NULL OR account_id = ANY($3))
         ORDER BY created_at, account_id",
        scope.owner.as_uuid(),
        groups,
        filter.as_deref(),
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut by_group: HashMap<Uuid, Vec<AccountId>> = HashMap::new();
    for row in rows {
        by_group
            .entry(row.group_id)
            .or_default()
            .push(AccountId::from_uuid(row.account_id));
    }
    Ok(by_group)
}
