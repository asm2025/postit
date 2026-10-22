//! Media asset rows.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. Media is not bound to an account,
//! so a delegate's account limit does not hide it.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{MediaAssetId, UserId};
use serde_json::Value;
use sqlx::PgConnection;

use crate::constraint::{is_foreign_key_violation, translate};
use crate::error::DataError;
use crate::paging;
use crate::text::{parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// Where a media asset is in its upload and processing life.
    MediaStatus {
        /// Bytes are still arriving.
        Uploading => "uploading",
        /// All bytes arrived; the file is being probed or fitted.
        Processing => "processing",
        /// Stored and usable.
        Ready => "ready",
        /// Upload or processing failed.
        Failed => "failed",
    }
}

/// A stored media asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// The asset.
    pub id: MediaAssetId,
    /// Where the bytes live in the media store.
    pub storage_key: String,
    /// Where the thumbnail lives, once there is one.
    pub thumbnail_key: Option<String>,
    /// The name the owner's file had.
    pub original_filename: Option<String>,
    /// The MIME type.
    pub mime: String,
    /// The stored size in bytes.
    pub bytes: i64,
    /// The SHA-256 of the stored bytes, once known.
    pub sha256: Option<Vec<u8>>,
    /// Pixel width, for images and video.
    pub width: Option<i32>,
    /// Pixel height, for images and video.
    pub height: Option<i32>,
    /// Duration, for video.
    pub duration_ms: Option<i64>,
    /// Where it is in its life.
    pub status: MediaStatus,
    /// The asset this one was fitted from.
    pub derived_from: Option<MediaAssetId>,
    /// The fitting that produced it.
    pub fit_spec: Option<Value>,
    /// The size the upload declared, while it is open.
    pub declared_bytes: Option<i64>,
    /// When an open upload session expires.
    pub upload_expires_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct MediaRow {
    id: uuid::Uuid,
    storage_key: String,
    thumbnail_key: Option<String>,
    original_filename: Option<String>,
    mime: String,
    bytes: i64,
    sha256: Option<Vec<u8>>,
    width: Option<i32>,
    height: Option<i32>,
    duration_ms: Option<i64>,
    status: String,
    derived_from: Option<uuid::Uuid>,
    fit_spec: Option<Value>,
    declared_bytes: Option<i64>,
    upload_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<MediaRow> for Media {
    type Error = DataError;

    fn try_from(row: MediaRow) -> Result<Self, DataError> {
        Ok(Self {
            id: MediaAssetId::from_uuid(row.id),
            storage_key: row.storage_key,
            thumbnail_key: row.thumbnail_key,
            original_filename: row.original_filename,
            mime: row.mime,
            bytes: row.bytes,
            sha256: row.sha256,
            width: row.width,
            height: row.height,
            duration_ms: row.duration_ms,
            status: parse(&row.status)?,
            derived_from: row.derived_from.map(MediaAssetId::from_uuid),
            fit_spec: row.fit_spec,
            declared_bytes: row.declared_bytes,
            upload_expires_at: row.upload_expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// A media asset to record.
#[derive(Debug, Clone)]
pub struct NewMedia {
    /// The asset's ID.
    pub id: MediaAssetId,
    /// Where the bytes live in the media store.
    pub storage_key: String,
    /// The name the owner's file had.
    pub original_filename: Option<String>,
    /// The MIME type.
    pub mime: String,
    /// The stored size in bytes so far.
    pub bytes: i64,
    /// Where it starts.
    pub status: MediaStatus,
    /// The size the upload declared.
    pub declared_bytes: Option<i64>,
    /// When an open upload session expires.
    pub upload_expires_at: Option<DateTime<Utc>>,
    /// The asset this one is fitted from.
    pub derived_from: Option<MediaAssetId>,
    /// The fitting that produces it.
    pub fit_spec: Option<Value>,
}

/// What a finished upload or fitting learned about the stored file.
#[derive(Debug, Clone)]
pub struct ReadyMedia {
    /// The stored size in bytes.
    pub bytes: i64,
    /// The SHA-256 of the stored bytes.
    pub sha256: Vec<u8>,
    /// Pixel width, for images and video.
    pub width: Option<i32>,
    /// Pixel height, for images and video.
    pub height: Option<i32>,
    /// Duration, for video.
    pub duration_ms: Option<i64>,
    /// Where the thumbnail lives.
    pub thumbnail_key: Option<String>,
}

/// Media assets.
pub struct MediaRepo;

impl MediaRepo {
    /// Records a media asset.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when `derived_from` is not the owner's asset, and
    /// [`DataError::Sql`] on a database failure.
    pub async fn insert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewMedia,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            MediaRow,
            "INSERT INTO media_assets
                 (id, owner_id, storage_key, original_filename, mime, bytes, status,
                  declared_bytes, upload_expires_at, derived_from, fit_spec,
                  created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_expires_at, created_at, updated_at",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.storage_key,
            new.original_filename,
            new.mime,
            new.bytes,
            new.status.as_str(),
            new.declared_bytes,
            new.upload_expires_at,
            new.derived_from.map(|id| id.as_uuid()),
            new.fit_spec,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| {
            if is_foreign_key_violation(&err) {
                DataError::NotFound
            } else {
                translate(err)
            }
        })?
        .try_into()
    }

    /// One asset.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<Media, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The owner's assets, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<Media>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE owner_id = $1
             ORDER BY created_at DESC, id DESC
             LIMIT $2 OFFSET $3",
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM media_assets WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(Media::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Marks an asset ready and records what was learned about the stored file. It closes the
    /// upload session.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_not_pending`) when the asset is already ready or failed.
    pub async fn set_ready(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
        ready: ReadyMedia,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query_as!(
            MediaRow,
            "UPDATE media_assets
             SET status = 'ready', bytes = $3, sha256 = $4, width = $5, height = $6,
                 duration_ms = $7, thumbnail_key = $8, upload_expires_at = NULL,
                 upload_offset = NULL, updated_at = now(), updated_by = $9
             WHERE owner_id = $1 AND id = $2 AND status IN ('uploading', 'processing')
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_expires_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            ready.bytes,
            ready.sha256,
            ready.width,
            ready.height,
            ready.duration_ms,
            ready.thumbnail_key,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(row) = updated {
            return row.try_into();
        }
        Self::get(conn, scope, id).await?;
        Err(DataError::Conflict("media_not_pending".to_string()))
    }

    /// Deletes an asset row. The caller removes the stored bytes.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_in_use`) while a post references it.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let deleted = sqlx::query!(
            "DELETE FROM media_assets WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await
        .map_err(|err| {
            if is_foreign_key_violation(&err) {
                DataError::Conflict("media_in_use".to_string())
            } else {
                DataError::Sql(err)
            }
        })?
        .rows_affected();
        if deleted == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// The bytes `owner` has stored: ready assets plus the declared size of open uploads.
    /// This is a system read for quota checks, not an access check.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn stored_bytes(conn: &mut PgConnection, owner: UserId) -> Result<i64, DataError> {
        let total = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(CASE status
                                      WHEN 'ready' THEN bytes
                                      WHEN 'uploading' THEN COALESCE(declared_bytes, 0)
                                      ELSE 0
                                  END), 0)::bigint AS "total!"
               FROM media_assets WHERE owner_id = $1"#,
            owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(total)
    }
}
