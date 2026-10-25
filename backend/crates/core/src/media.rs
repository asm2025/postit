//! Media assets and the traits that store, probe, and host them.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{MediaAssetId, UserId};

/// A stream of media bytes. Media is always streamed, never buffered whole.
pub type ByteStream = futures::stream::BoxStream<'static, std::io::Result<bytes::Bytes>>;

/// What a media asset is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// A still image.
    Image,
    /// A video.
    Video,
}

/// Where a media asset is in its upload and processing life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MediaAssetState {
    /// Bytes are still arriving.
    Uploading,
    /// Stored, probed, and usable.
    Ready,
    /// Upload or processing failed.
    Failed,
}

/// A media file the owner uploaded (or one derived from it by fitting).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MediaAsset {
    /// Asset ID.
    pub id: MediaAssetId,
    /// The user who owns the asset.
    pub owner_id: UserId,
    /// Image or video.
    pub kind: MediaKind,
    /// MIME type, e.g. `video/mp4`.
    pub mime: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Width in pixels, when probed.
    pub width: Option<u32>,
    /// Height in pixels, when probed.
    pub height: Option<u32>,
    /// Duration in milliseconds (video), when probed.
    pub duration_ms: Option<u64>,
    /// Lowercase hex SHA-256 of the content, when computed.
    pub sha256: Option<String>,
    /// Upload and processing state.
    pub state: MediaAssetState,
    /// The original this asset was derived from by fitting, if any.
    pub derived_from: Option<MediaAssetId>,
}

impl MediaAsset {
    /// Width divided by height, when both are known and non-zero.
    #[must_use]
    pub fn aspect(&self) -> Option<f64> {
        match (self.width, self.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some(f64::from(w) / f64::from(h)),
            _ => None,
        }
    }

    /// Whether the asset is taller than wide. `false` when dimensions are unknown.
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        matches!((self.width, self.height), (Some(w), Some(h)) if h > w)
    }
}

/// Where an agent can get a media asset's bytes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSource {
    /// A stored asset, read through [`MediaStore`].
    Stored {
        /// The asset ID.
        id: MediaAssetId,
    },
    /// A public URL the platform fetches itself.
    PublicUrl {
        /// The URL.
        url: url::Url,
    },
    /// A platform-side reference (e.g. a file ID the platform already holds).
    RemoteRef {
        /// The platform's reference.
        reference: String,
    },
}

/// What probing a file found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProbeInfo {
    /// Width in pixels.
    pub width: Option<u32>,
    /// Height in pixels.
    pub height: Option<u32>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Why a media operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// The asset does not exist.
    #[error("media asset not found")]
    NotFound,
    /// The asset is not in the `ready` state.
    #[error("media asset is not ready")]
    NotReady,
    /// Reading or writing failed.
    #[error("media I/O failed: {0}")]
    Io(String),
    /// The file type or codec is not supported.
    #[error("unsupported media: {0}")]
    Unsupported(String),
}

/// Opens stored media for streaming.
#[async_trait]
pub trait MediaStore: Send + Sync {
    /// Streams the asset's bytes.
    ///
    /// # Errors
    /// [`MediaError::NotFound`] or [`MediaError::NotReady`] when the asset cannot be read.
    async fn open(&self, asset: &MediaAsset) -> Result<ByteStream, MediaError>;
}

/// Reads dimensions and duration from a media file.
#[async_trait]
pub trait MediaProbe: Send + Sync {
    /// Probes the file at `path`.
    ///
    /// # Errors
    /// [`MediaError::Unsupported`] when the file is not recognized media.
    async fn probe(&self, path: &Path, mime: &str) -> Result<ProbeInfo, MediaError>;
}

/// Makes stored media reachable by URL for platforms that fetch it themselves.
pub trait MediaHost: Send + Sync {
    /// A URL that serves `asset` for at most `ttl`.
    ///
    /// # Errors
    /// [`MediaError::NotReady`] when the asset is not ready to serve.
    fn public_url(&self, asset: &MediaAsset, ttl: Duration) -> Result<url::Url, MediaError>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn video() -> MediaAsset {
        MediaAsset {
            id: MediaAssetId::from_uuid(Uuid::nil()),
            owner_id: UserId::from_uuid(Uuid::nil()),
            kind: MediaKind::Video,
            mime: "video/mp4".into(),
            bytes: 1_000,
            width: Some(1080),
            height: Some(1920),
            duration_ms: Some(15_000),
            sha256: Some("ab".repeat(32)),
            state: MediaAssetState::Ready,
            derived_from: None,
        }
    }

    #[test]
    fn asset_round_trips() {
        let a = video();
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["kind"], "video");
        assert_eq!(json["state"], "ready");
        assert_eq!(serde_json::from_value::<MediaAsset>(json).unwrap(), a);
    }

    #[test]
    fn aspect_and_orientation() {
        let a = video();
        assert!((a.aspect().unwrap() - 0.5625).abs() < 1e-9);
        assert!(a.is_vertical());
        let unknown = MediaAsset {
            width: None,
            height: None,
            ..video()
        };
        assert_eq!(unknown.aspect(), None);
        assert!(!unknown.is_vertical());
        let zero = MediaAsset {
            width: Some(0),
            height: Some(0),
            ..video()
        };
        assert_eq!(zero.aspect(), None);
    }

    #[test]
    fn media_source_wire_format() {
        let s = MediaSource::PublicUrl {
            url: "https://media.example/a".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({"type":"public_url","url":"https://media.example/a"})
        );
        let r = MediaSource::RemoteRef {
            reference: "file-1".into(),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type":"remote_ref","reference":"file-1"})
        );
    }
}
