//! Media asset rows.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. Media is not bound to an account,
//! so a delegate's account limit does not hide it.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{MediaAssetId, UserId};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

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
    /// How many bytes of an open upload are stored.
    pub upload_offset: Option<i64>,
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
    upload_offset: Option<i64>,
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
            upload_offset: row.upload_offset,
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
    /// The MIME type the stored bytes sniffed as.
    pub mime: String,
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

/// What the garbage collector learns about a row it deleted.
#[derive(Debug, Clone)]
pub struct GcRow {
    /// The deleted asset.
    pub id: MediaAssetId,
    /// Where its bytes lived.
    pub storage_key: String,
    /// Where its thumbnail lived.
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
                       declared_bytes, upload_offset, upload_expires_at, created_at, updated_at",
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
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
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

    /// The assets `ids`, in the order asked for, in one query.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when any of them is not the owner's asset.
    pub async fn get_many(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        ids: &[MediaAssetId],
    ) -> Result<Vec<Media>, DataError> {
        scope.require(Capability::View)?;
        let wanted: Vec<uuid::Uuid> = ids.iter().map(MediaAssetId::as_uuid).collect();
        let rows = sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE owner_id = $1 AND id = ANY($2)",
            scope.owner.as_uuid(),
            &wanted,
        )
        .fetch_all(&mut *conn)
        .await?;
        let mut found = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let media = Media::try_from(row)?;
            found.insert(media.id, media);
        }
        ids.iter()
            .map(|id| found.get(id).cloned().ok_or(DataError::NotFound))
            .collect()
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
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
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
                 upload_offset = NULL, mime = $10, updated_at = now(), updated_by = $9
             WHERE owner_id = $1 AND id = $2 AND status IN ('uploading', 'processing')
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_offset, upload_expires_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            ready.bytes,
            ready.sha256,
            ready.width,
            ready.height,
            ready.duration_ms,
            ready.thumbnail_key,
            scope.actor.as_uuid(),
            ready.mime,
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(row) = updated {
            return row.try_into();
        }
        Self::get(conn, scope, id).await?;
        Err(DataError::Conflict("media_not_pending".to_string()))
    }

    /// Deletes an asset row and returns it, so the caller can remove the stored bytes.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_in_use`) while a post or the snapshot of a job that has not ended references it.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        // A job that has not ended publishes its frozen snapshot, which names the asset
        // whether or not the post still does. A job created after this check is built from a
        // post whose `post_media` row still links the asset, so the delete below fails on that
        // foreign key instead.
        let frozen = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM publish_jobs
                   WHERE owner_id = $1 AND status IN ('scheduled', 'publishing')
                     AND jsonb_path_exists(
                         content_snapshot,
                         '$.variants[*].media[*].asset ? (@ == $asset)',
                         jsonb_build_object('asset', $2::text))
               ) AS "frozen!""#,
            scope.owner.as_uuid(),
            id.to_string(),
        )
        .fetch_one(&mut *conn)
        .await?;
        if frozen {
            return Err(DataError::Conflict("media_in_use".to_string()));
        }
        sqlx::query_as!(
            MediaRow,
            "DELETE FROM media_assets WHERE owner_id = $1 AND id = $2
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_offset, upload_expires_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(|err| {
            if is_foreign_key_violation(&err) {
                DataError::Conflict("media_in_use".to_string())
            } else {
                DataError::Sql(err)
            }
        })?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Reserves storage for an upload: locks the owner's quota row, refuses when the stored
    /// bytes plus `new.declared_bytes` would pass the quota (`0` means unlimited), and records
    /// the open session with `upload_offset = 0`, all in one transaction. Owners with no limits
    /// row are unlimited.
    ///
    /// # Errors
    ///
    /// [`DataError::StorageQuotaExceeded`] over the quota, and [`DataError::Conflict`]
    /// (`declared_bytes_required`) when `new.declared_bytes` is unset.
    pub async fn reserve(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewMedia,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        let declared = new
            .declared_bytes
            .ok_or_else(|| DataError::Conflict("declared_bytes_required".to_string()))?;
        let mut tx = conn.begin().await?;
        let quota = sqlx::query_scalar!(
            "SELECT storage_quota_bytes FROM user_limits WHERE user_id = $1 FOR UPDATE",
            scope.owner.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or(0);
        if quota > 0 {
            let used = Self::stored_bytes(&mut tx, scope.owner).await?;
            if used.saturating_add(declared) > quota {
                return Err(DataError::StorageQuotaExceeded { quota, used });
            }
        }
        let media = sqlx::query_as!(
            MediaRow,
            "INSERT INTO media_assets
                 (id, owner_id, storage_key, original_filename, mime, bytes, status,
                  declared_bytes, upload_offset, upload_expires_at, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, 0, 'uploading', $6, 0, $7, $8, $8)
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_offset, upload_expires_at, created_at, updated_at",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.storage_key,
            new.original_filename,
            new.mime,
            declared,
            new.upload_expires_at,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(translate)?
        .try_into()?;
        tx.commit().await?;
        Ok(media)
    }

    /// Locks an open, unexpired upload session until the caller's transaction ends. The caller
    /// holds a transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such open session.
    pub async fn lock_session(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
        now: DateTime<Utc>,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE owner_id = $1 AND id = $2 AND status = 'uploading' AND upload_expires_at > $3
             FOR UPDATE",
            scope.owner.as_uuid(),
            id.as_uuid(),
            now,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// Records how many bytes of an open upload are stored.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such open session.
    pub async fn set_offset(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
        offset: i64,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query!(
            "UPDATE media_assets
             SET upload_offset = $3, bytes = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2 AND status = 'uploading'",
            scope.owner.as_uuid(),
            id.as_uuid(),
            offset,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Moves an open upload to `processing` and counts its declared size as stored, so the
    /// quota still holds those bytes while the file is checked.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_not_pending`) when it is not an open upload.
    pub async fn set_processing(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<Media, DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query_as!(
            MediaRow,
            "UPDATE media_assets
             SET status = 'processing', bytes = COALESCE(declared_bytes, bytes),
                 updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2 AND status = 'uploading'
             RETURNING id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                       width, height, duration_ms, status, derived_from, fit_spec,
                       declared_bytes, upload_offset, upload_expires_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
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

    /// Moves a `processing` upload back to an open upload, so a completion that failed for a
    /// reason other than its content can be asked for again.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_not_pending`) when it is not processing.
    pub async fn reopen(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query!(
            "UPDATE media_assets
             SET status = 'uploading', updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2 AND status = 'processing'",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            Self::get(conn, scope, id).await?;
            return Err(DataError::Conflict("media_not_pending".to_string()));
        }
        Ok(())
    }

    /// Marks an asset that is not ready as failed.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such asset, and [`DataError::Conflict`]
    /// (`media_not_pending`) when it is already ready.
    pub async fn set_failed(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query!(
            "UPDATE media_assets
             SET status = 'failed', updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2 AND status <> 'ready'",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            Self::get(conn, scope, id).await?;
            return Err(DataError::Conflict("media_not_pending".to_string()));
        }
        Ok(())
    }

    /// A ready asset of `owner`. This is a system read for serving and agents, not an access
    /// check: the caller has already proven the right to the owner's media.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn find_ready(
        conn: &mut PgConnection,
        owner: UserId,
        id: MediaAssetId,
    ) -> Result<Option<Media>, DataError> {
        sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE owner_id = $1 AND id = $2 AND status = 'ready'",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Media::try_from)
        .transpose()
    }

    /// A ready asset, found by ID alone. This is a system read for the signed public URL,
    /// whose signature is the access proof.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn find_ready_public(
        conn: &mut PgConnection,
        id: MediaAssetId,
    ) -> Result<Option<Media>, DataError> {
        sqlx::query_as!(
            MediaRow,
            "SELECT id, storage_key, thumbnail_key, original_filename, mime, bytes, sha256,
                    width, height, duration_ms, status, derived_from, fit_spec,
                    declared_bytes, upload_offset, upload_expires_at, created_at, updated_at
             FROM media_assets
             WHERE id = $1 AND status = 'ready'",
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(Media::try_from)
        .transpose()
    }

    /// Deletes up to `limit` rows no one will finish or use: failed assets, open uploads
    /// expired before `now`, and assets stuck in `processing` since before `stale_before`.
    /// Returns their keys so the caller can remove the files. This is a system job, not an
    /// access check. Assets a post still links are left alone.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_garbage(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<GcRow>, DataError> {
        let rows = sqlx::query!(
            "DELETE FROM media_assets
             WHERE id IN (
                 SELECT m.id FROM media_assets m
                 WHERE (m.status = 'failed'
                        OR (m.status = 'uploading' AND m.upload_expires_at < $1)
                        OR (m.status = 'processing' AND m.updated_at < $2))
                   AND NOT EXISTS (SELECT 1 FROM post_media pm WHERE pm.media_id = m.id)
                 ORDER BY m.created_at
                 LIMIT $3
                 FOR UPDATE OF m SKIP LOCKED)
             RETURNING id, storage_key, thumbnail_key",
            now,
            stale_before,
            limit,
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| GcRow {
                id: MediaAssetId::from_uuid(row.id),
                storage_key: row.storage_key,
                thumbnail_key: row.thumbnail_key,
            })
            .collect())
    }

    /// The subset of `keys` that some row names as its `storage_key` or `thumbnail_key`.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn known_keys(
        conn: &mut PgConnection,
        keys: &[String],
    ) -> Result<HashSet<String>, DataError> {
        let found = sqlx::query_scalar!(
            r#"SELECT k AS "k!" FROM (
                   SELECT storage_key AS k FROM media_assets WHERE storage_key = ANY($1)
                   UNION
                   SELECT thumbnail_key FROM media_assets WHERE thumbnail_key = ANY($1)
               ) t"#,
            keys,
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(found.into_iter().collect())
    }

    /// The bytes `owner` has stored: ready and processing assets plus the declared size of
    /// open uploads.
    /// This is a system read for quota checks, not an access check.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn stored_bytes(conn: &mut PgConnection, owner: UserId) -> Result<i64, DataError> {
        let total = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(CASE status
                                      WHEN 'ready' THEN bytes
                                      WHEN 'processing' THEN bytes
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
