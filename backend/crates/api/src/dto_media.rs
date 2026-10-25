//! Wire types of the media routes. No DTO carries a storage key or a path: assets are built
//! field by field.

use chrono::{DateTime, Utc};
use postit_core::MediaKind;
use postit_data::media::{Media, MediaStatus};
use postit_media::{Usage, ViewUrls};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// What a media asset is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaKindDto {
    /// A still image.
    Image,
    /// A video.
    Video,
}

/// Where a media asset is in its upload and processing life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatusDto {
    /// Bytes are still arriving.
    Uploading,
    /// All bytes arrived; the file is being checked.
    Processing,
    /// Stored and usable.
    Ready,
    /// The upload was refused or failed.
    Failed,
}

impl From<MediaStatus> for MediaStatusDto {
    fn from(status: MediaStatus) -> Self {
        match status {
            MediaStatus::Uploading => Self::Uploading,
            MediaStatus::Processing => Self::Processing,
            MediaStatus::Ready => Self::Ready,
            MediaStatus::Failed => Self::Failed,
        }
    }
}

/// A media asset.
#[derive(Debug, Serialize, ToSchema)]
pub struct MediaDto {
    /// Asset ID.
    pub id: Uuid,
    /// Image or video.
    pub kind: MediaKindDto,
    /// The MIME type the bytes sniffed as; unset until the upload completes.
    pub mime: Option<String>,
    /// The stored size in bytes.
    pub bytes: u64,
    /// Pixel width, once probed.
    pub width: Option<u32>,
    /// Pixel height, once probed.
    pub height: Option<u32>,
    /// Duration in milliseconds, for video.
    pub duration_ms: Option<u64>,
    /// Lowercase hex SHA-256 of the stored bytes, once known.
    pub sha256: Option<String>,
    /// Where it is in its life.
    pub status: MediaStatusDto,
    /// The name the uploaded file had. Metadata only; it never reaches a path.
    pub original_filename: Option<String>,
    /// Whether `GET /media/{id}/thumbnail` has an image.
    pub has_thumbnail: bool,
    /// The asset this one was fitted from.
    pub derived_from: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl From<Media> for MediaDto {
    fn from(media: Media) -> Self {
        let ready = media.status == MediaStatus::Ready;
        let kind = if media.mime.starts_with("video/") {
            MediaKind::Video
        } else if media.mime.starts_with("image/") {
            MediaKind::Image
        } else {
            media
                .original_filename
                .as_deref()
                .and_then(postit_media::kind_for_filename)
                .unwrap_or(MediaKind::Image)
        };
        Self {
            id: media.id.as_uuid(),
            kind: match kind {
                MediaKind::Image => MediaKindDto::Image,
                MediaKind::Video => MediaKindDto::Video,
            },
            mime: ready.then_some(media.mime),
            bytes: u64::try_from(media.bytes).unwrap_or(0),
            width: media.width.and_then(|v| u32::try_from(v).ok()),
            height: media.height.and_then(|v| u32::try_from(v).ok()),
            duration_ms: media.duration_ms.and_then(|v| u64::try_from(v).ok()),
            sha256: media.sha256.as_deref().map(hex::encode),
            status: media.status.into(),
            original_filename: media.original_filename,
            has_thumbnail: media.thumbnail_key.is_some(),
            derived_from: media.derived_from.map(|id| id.as_uuid()),
            created_at: media.created_at,
        }
    }
}

/// Starts a resumable upload.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadSessionRequest {
    /// The file name, 1 to 255 characters. Its extension (`jpg`, `png`, `gif`, `webp`, `mp4`,
    /// `m4v`, `mov`) picks the kind and so the size limit.
    pub filename: String,
    /// The whole file's size in bytes.
    pub size: u64,
}

/// An open upload.
#[derive(Debug, Serialize, ToSchema)]
pub struct UploadSessionDto {
    /// The asset ID the upload becomes.
    pub id: Uuid,
    /// Bytes stored so far.
    pub offset: u64,
    /// The declared size.
    pub size: u64,
    /// When the session expires if left open.
    pub expires_at: Option<DateTime<Utc>>,
    /// Largest chunk one `PUT` may carry.
    pub chunk_max: u64,
}

impl UploadSessionDto {
    /// The session of an open `media` row.
    #[must_use]
    pub fn new(media: &Media, chunk_max: u64) -> Self {
        Self {
            id: media.id.as_uuid(),
            offset: u64::try_from(media.upload_offset.unwrap_or(0)).unwrap_or(0),
            size: u64::try_from(media.declared_bytes.unwrap_or(0)).unwrap_or(0),
            expires_at: media.upload_expires_at,
            chunk_max,
        }
    }
}

/// Where an upload stands after a chunk.
#[derive(Debug, Serialize, ToSchema)]
pub struct UploadOffsetDto {
    /// Bytes stored so far; the next chunk starts here.
    pub offset: u64,
}

/// Storage used against the quota.
#[derive(Debug, Serialize, ToSchema)]
pub struct MediaUsageDto {
    /// Bytes stored or reserved by open uploads.
    pub used_bytes: u64,
    /// The quota in bytes; 0 means unlimited.
    pub quota_bytes: u64,
}

impl From<Usage> for MediaUsageDto {
    fn from(usage: Usage) -> Self {
        Self {
            used_bytes: usage.used_bytes,
            quota_bytes: usage.quota_bytes,
        }
    }
}

/// Short-lived signed URLs for showing an asset.
#[derive(Debug, Serialize, ToSchema)]
pub struct ViewUrlsDto {
    /// The file.
    pub content_url: String,
    /// Its thumbnail, when it has one.
    pub thumbnail_url: Option<String>,
    /// When both stop working.
    pub expires_at: DateTime<Utc>,
}

impl From<ViewUrls> for ViewUrlsDto {
    fn from(urls: ViewUrls) -> Self {
        Self {
            content_url: urls.content_url.into(),
            thumbnail_url: urls.thumbnail_url.map(Into::into),
            expires_at: urls.expires_at,
        }
    }
}
