//! Campaigns: named groups of posts with an optional date range.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. Campaigns are not bound to an
//! account, so a delegate's account limit does not hide them.

use chrono::{DateTime, NaiveDate, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::CampaignId;
use sqlx::PgConnection;

use crate::error::DataError;
use crate::paging;
use crate::{Capability, OwnerScope};

/// A campaign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    /// The campaign.
    pub id: CampaignId,
    /// Its name.
    pub name: String,
    /// The first day it runs.
    pub starts_on: Option<NaiveDate>,
    /// The last day it runs.
    pub ends_on: Option<NaiveDate>,
    /// A display color, as the client chose it.
    pub color: Option<String>,
    /// Whether it is hidden from the default list.
    pub archived: bool,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct CampaignRow {
    id: uuid::Uuid,
    name: String,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
    color: Option<String>,
    archived: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CampaignRow> for Campaign {
    fn from(row: CampaignRow) -> Self {
        Self {
            id: CampaignId::from_uuid(row.id),
            name: row.name,
            starts_on: row.starts_on,
            ends_on: row.ends_on,
            color: row.color,
            archived: row.archived,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// A campaign to create, or the fields an update replaces.
#[derive(Debug, Clone)]
pub struct CampaignFields {
    /// Its name.
    pub name: String,
    /// The first day it runs.
    pub starts_on: Option<NaiveDate>,
    /// The last day it runs; not before `starts_on`.
    pub ends_on: Option<NaiveDate>,
    /// A display color.
    pub color: Option<String>,
}

/// Campaigns.
pub struct CampaignRepo;

impl CampaignRepo {
    /// Creates a campaign.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] when `ends_on` is before `starts_on`, or on a database failure.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: CampaignId,
        fields: CampaignFields,
    ) -> Result<Campaign, DataError> {
        scope.require(Capability::Edit)?;
        let row = sqlx::query_as!(
            CampaignRow,
            "INSERT INTO campaigns
                 (id, owner_id, name, starts_on, ends_on, color, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
             RETURNING id, name, starts_on, ends_on, color, archived, created_at, updated_at",
            id.as_uuid(),
            scope.owner.as_uuid(),
            fields.name,
            fields.starts_on,
            fields.ends_on,
            fields.color,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.into())
    }

    /// One campaign.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such campaign.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: CampaignId,
    ) -> Result<Campaign, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            CampaignRow,
            "SELECT id, name, starts_on, ends_on, color, archived, created_at, updated_at
             FROM campaigns WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Campaign::from)
        .ok_or(DataError::NotFound)
    }

    /// The owner's campaigns, newest first. Archived ones appear only when asked for.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
        include_archived: bool,
    ) -> Result<ResultSet<Campaign>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            CampaignRow,
            "SELECT id, name, starts_on, ends_on, color, archived, created_at, updated_at
             FROM campaigns
             WHERE owner_id = $1 AND ($2 OR NOT archived)
             ORDER BY created_at DESC, id DESC
             LIMIT $3 OFFSET $4",
            scope.owner.as_uuid(),
            include_archived,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM campaigns WHERE owner_id = $1 AND ($2 OR NOT archived)",
            scope.owner.as_uuid(),
            include_archived,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows.into_iter().map(Campaign::from).collect(),
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Replaces a campaign's name, dates, and color.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such campaign, and [`DataError::Sql`] when
    /// `ends_on` is before `starts_on`.
    pub async fn update(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: CampaignId,
        fields: CampaignFields,
    ) -> Result<Campaign, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            CampaignRow,
            "UPDATE campaigns
             SET name = $3, starts_on = $4, ends_on = $5, color = $6,
                 updated_at = now(), updated_by = $7
             WHERE owner_id = $1 AND id = $2
             RETURNING id, name, starts_on, ends_on, color, archived, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            fields.name,
            fields.starts_on,
            fields.ends_on,
            fields.color,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Campaign::from)
        .ok_or(DataError::NotFound)
    }

    /// Archives or restores a campaign.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such campaign.
    pub async fn archive(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: CampaignId,
        archived: bool,
    ) -> Result<Campaign, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            CampaignRow,
            "UPDATE campaigns
             SET archived = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2
             RETURNING id, name, starts_on, ends_on, color, archived, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            archived,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Campaign::from)
        .ok_or(DataError::NotFound)
    }

    /// Deletes a campaign. Its posts stay and lose the campaign.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such campaign.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: CampaignId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let deleted = sqlx::query!(
            "DELETE FROM campaigns WHERE owner_id = $1 AND id = $2",
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
