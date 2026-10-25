//! The media service: the one entry point the API calls.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_config::MediaSettings;
use postit_core::{
    ByteStream, Clock, IdGenerator, MediaAsset, MediaAssetId, MediaAssetState, MediaKind,
    MediaProbe, ProbeInfo, UserId,
};
use postit_data::limits::UserLimitsRepo;
use postit_data::media::{Media, MediaRepo, MediaStatus, NewMedia, ReadyMedia};
use postit_data::{Capability, DataError, OwnerScope};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use url::Url;

use crate::error::MediaServiceError;
use crate::intake::{IntakeError, Written, copy_stream};
use crate::signing::{SignedTarget, UrlSigner};
use crate::sniff::{SNIFF_LEN, kind_for_filename, sniff};
use crate::storage::{FsMediaStore, StorageKey};
use crate::thumbnail::Thumbnailer;

/// The mime recorded for an upload until its bytes are sniffed.
const PENDING_MIME: &str = "application/octet-stream";
const LONGEST_FILENAME: usize = 255;

/// Everything the service needs, built once by the composition root.
pub struct MediaServiceDeps {
    /// The database.
    pub pool: PgPool,
    /// Where the bytes live.
    pub store: Arc<FsMediaStore>,
    /// Reads dimensions and duration.
    pub probe: Arc<dyn MediaProbe>,
    /// Makes thumbnails.
    pub thumbnailer: Thumbnailer,
    /// Signs view and public URLs.
    pub signer: Arc<UrlSigner>,
    /// The time source.
    pub clock: Arc<dyn Clock>,
    /// The ID source.
    pub ids: Arc<dyn IdGenerator>,
    /// Limits, TTLs, and timeouts.
    pub settings: MediaSettings,
}

/// Uploads, reads, signs, and deletes media.
#[derive(Clone)]
pub struct MediaService {
    pub(crate) d: Arc<MediaServiceDeps>,
}

fn invalid(field: &'static str) -> MediaServiceError {
    MediaServiceError::Invalid { field }
}

fn to_i64(value: u64) -> Result<i64, MediaServiceError> {
    i64::try_from(value).map_err(|_| invalid("size"))
}

/// Trims `raw`, checks it is a usable name, and reads the kind its extension claims.
fn check_filename(raw: &str) -> Result<(String, MediaKind), MediaServiceError> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > LONGEST_FILENAME
        || name.chars().any(char::is_control)
    {
        return Err(invalid("filename"));
    }
    let kind = kind_for_filename(name).ok_or(MediaServiceError::UnsupportedMediaType)?;
    Ok((name.to_string(), kind))
}

impl MediaService {
    /// Builds the service.
    #[must_use]
    pub fn new(deps: MediaServiceDeps) -> Self {
        Self { d: Arc::new(deps) }
    }

    fn kind_max(&self, kind: MediaKind) -> u64 {
        match kind {
            MediaKind::Image => self.d.settings.image_max_bytes,
            MediaKind::Video => self.d.settings.video_max_bytes,
        }
    }

    /// Reserves quota and records an open upload session for a file of `size` bytes.
    async fn open_session(
        &self,
        scope: &OwnerScope,
        name: String,
        size: u64,
    ) -> Result<(Media, StorageKey), MediaServiceError> {
        let now = self.d.clock.now();
        let ttl = chrono::Duration::from_std(self.d.settings.upload_session_ttl)
            .map_err(|_| invalid("upload_session_ttl"))?;
        let id = MediaAssetId::from_uuid(self.d.ids.generate());
        let key = StorageKey::new(scope.owner, id, now);
        let new = NewMedia {
            id,
            storage_key: key.as_str().to_string(),
            original_filename: Some(name),
            mime: PENDING_MIME.to_string(),
            bytes: 0,
            status: MediaStatus::Uploading,
            declared_bytes: Some(to_i64(size)?),
            upload_expires_at: Some(now + ttl),
            derived_from: None,
            fit_spec: None,
        };
        let mut conn = self.d.pool.acquire().await?;
        let media = MediaRepo::reserve(&mut conn, scope, new).await?;
        Ok((media, key))
    }

    /// Marks the asset failed and removes its files. Errors are logged, never returned: the
    /// caller is already reporting the failure that got us here.
    async fn discard(&self, scope: &OwnerScope, id: MediaAssetId, key: &StorageKey) {
        match self.d.pool.acquire().await {
            Ok(mut conn) => {
                if let Err(err) = MediaRepo::set_failed(&mut conn, scope, id).await {
                    tracing::warn!(error = %err, "could not mark a failed upload");
                }
            }
            Err(err) => tracing::warn!(error = %err, "could not mark a failed upload"),
        }
        for k in [key.clone(), key.thumbnail()] {
            let Ok(path) = self.d.store.resolve(&k) else {
                continue;
            };
            if let Err(err) = tokio::fs::remove_file(&path).await
                && err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(key = %k.as_str(), error = %err, "could not remove a failed upload");
            }
        }
    }

    /// One request carrying the whole file.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::Invalid`] for a bad name or size, or a body that does not match
    /// `declared`; [`MediaServiceError::UnsupportedMediaType`] for a file that is not an
    /// accepted image or video; [`MediaServiceError::PayloadTooLarge`] and
    /// [`MediaServiceError::StorageQuotaExceeded`] before any byte is stored.
    pub async fn upload(
        &self,
        scope: &OwnerScope,
        filename: &str,
        declared: u64,
        body: ByteStream,
    ) -> Result<Media, MediaServiceError> {
        scope
            .require(Capability::Edit)
            .map_err(MediaServiceError::Scope)?;
        let (name, kind) = check_filename(filename)?;
        if declared == 0 {
            return Err(invalid("size"));
        }
        let max = self.d.settings.single_request_max.min(self.kind_max(kind));
        if declared > max {
            return Err(MediaServiceError::PayloadTooLarge { max });
        }
        let (media, key) = self.open_session(scope, name, declared).await?;
        let outcome = async {
            let written = self.write_new(&key, declared, body).await?;
            let mut conn = self.d.pool.acquire().await?;
            MediaRepo::set_processing(&mut conn, scope, media.id).await?;
            drop(conn);
            self.finish(scope, &media, &key, kind, written).await
        }
        .await;
        if outcome.is_err() {
            self.discard(scope, media.id, &key).await;
        }
        outcome
    }

    /// Streams a single-request body into a new file.
    async fn write_new(
        &self,
        key: &StorageKey,
        declared: u64,
        body: ByteStream,
    ) -> Result<Written, MediaServiceError> {
        let path = self.d.store.resolve(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(&path).await?;
        let mut out = tokio::io::BufWriter::new(&mut file);
        let written =
            copy_stream(&mut out, body, declared, true)
                .await
                .map_err(|err| match err {
                    IntakeError::TooLong => invalid("content_length"),
                    IntakeError::Stream => MediaServiceError::BodyUnreadable,
                    IntakeError::Io(e) => MediaServiceError::Io(e),
                })?;
        if written.bytes != declared {
            return Err(invalid("content_length"));
        }
        Ok(written)
    }

    /// Starts a resumable upload of `size` bytes.
    ///
    /// # Errors
    ///
    /// The same refusals as [`Self::upload`], with the kind limit instead of the single-request
    /// limit.
    pub async fn start_session(
        &self,
        scope: &OwnerScope,
        filename: &str,
        size: u64,
    ) -> Result<Media, MediaServiceError> {
        scope
            .require(Capability::Edit)
            .map_err(MediaServiceError::Scope)?;
        let (name, kind) = check_filename(filename)?;
        if size == 0 {
            return Err(invalid("size"));
        }
        let max = self.kind_max(kind);
        if size > max {
            return Err(MediaServiceError::PayloadTooLarge { max });
        }
        let (media, _key) = self.open_session(scope, name, size).await?;
        Ok(media)
    }

    /// Appends the chunk `start..=end_inclusive` of a `total`-byte upload and returns the new
    /// offset. Concurrent appends to one upload serialize on its row; bytes a dropped
    /// connection left behind the stored offset are cut off first.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::UploadOffsetMismatch`] when `start` is not the stored offset,
    /// [`MediaServiceError::Invalid`] (`content_range`) for a range that does not fit the
    /// upload or the body, and [`MediaServiceError::NotFound`] for a missing or expired session.
    pub async fn append(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
        start: u64,
        end_inclusive: u64,
        total: u64,
        body: ByteStream,
    ) -> Result<u64, MediaServiceError> {
        scope
            .require(Capability::Edit)
            .map_err(MediaServiceError::Scope)?;
        if start > end_inclusive || end_inclusive >= total {
            return Err(invalid("content_range"));
        }
        let len = end_inclusive - start + 1;
        let chunk_max = self.d.settings.chunk_max;
        if len > chunk_max {
            return Err(MediaServiceError::PayloadTooLarge { max: chunk_max });
        }
        let mut tx = self.d.pool.begin().await?;
        let media = MediaRepo::lock_session(&mut tx, scope, id, self.d.clock.now()).await?;
        let offset = u64::try_from(media.upload_offset.unwrap_or(0)).unwrap_or(0);
        if media.declared_bytes.and_then(|d| u64::try_from(d).ok()) != Some(total) {
            return Err(invalid("content_range"));
        }
        if start != offset {
            return Err(MediaServiceError::UploadOffsetMismatch { offset });
        }
        let key = StorageKey::parse(&media.storage_key).ok_or(MediaServiceError::NotFound)?;
        let path = self.d.store.resolve(&key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .await?;
        if file.metadata().await?.len() < offset {
            return Err(MediaServiceError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "stored upload is shorter than its offset",
            )));
        }
        // Drop whatever a dropped connection left behind the committed offset.
        file.set_len(offset).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut out = tokio::io::BufWriter::new(&mut file);
        let written = copy_stream(&mut out, body, len, false)
            .await
            .map_err(|err| match err {
                IntakeError::TooLong => invalid("content_range"),
                IntakeError::Stream => MediaServiceError::BodyUnreadable,
                IntakeError::Io(e) => MediaServiceError::Io(e),
            })?;
        if written.bytes != len {
            return Err(invalid("content_range"));
        }
        let new_offset = offset + len;
        MediaRepo::set_offset(&mut tx, scope, id, to_i64(new_offset)?).await?;
        tx.commit().await?;
        Ok(new_offset)
    }

    /// The stored offset and declared size of an open upload.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when there is no open, unexpired session.
    pub async fn offset(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<(u64, u64), MediaServiceError> {
        let mut conn = self.d.pool.acquire().await?;
        let media = MediaRepo::get(&mut conn, scope, id).await?;
        let open = media.status == MediaStatus::Uploading
            && media
                .upload_expires_at
                .is_some_and(|expires| expires > self.d.clock.now());
        if !open {
            return Err(MediaServiceError::NotFound);
        }
        let offset = u64::try_from(media.upload_offset.unwrap_or(0)).unwrap_or(0);
        let declared = u64::try_from(media.declared_bytes.unwrap_or(0)).unwrap_or(0);
        Ok((offset, declared))
    }

    /// Closes an upload whose last byte arrived: hashes, sniffs, probes, makes the thumbnail,
    /// and marks the asset ready. Any refusal marks it failed and removes its files.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::UploadOffsetMismatch`] before the last byte,
    /// [`MediaServiceError::UnsupportedMediaType`] for bytes that are not the claimed kind of
    /// accepted media, and [`MediaServiceError::NotFound`] for a missing or expired session.
    pub async fn complete(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<Media, MediaServiceError> {
        scope
            .require(Capability::Edit)
            .map_err(MediaServiceError::Scope)?;
        let mut tx = self.d.pool.begin().await?;
        let locked = MediaRepo::lock_session(&mut tx, scope, id, self.d.clock.now()).await?;
        let declared = u64::try_from(locked.declared_bytes.unwrap_or(0)).unwrap_or(0);
        let offset = u64::try_from(locked.upload_offset.unwrap_or(0)).unwrap_or(0);
        if offset != declared {
            return Err(MediaServiceError::UploadOffsetMismatch { offset });
        }
        let media = MediaRepo::set_processing(&mut tx, scope, id).await?;
        tx.commit().await?;
        let key = StorageKey::parse(&media.storage_key).ok_or(MediaServiceError::NotFound)?;
        let outcome = async {
            let kind = media
                .original_filename
                .as_deref()
                .and_then(kind_for_filename)
                .ok_or(MediaServiceError::UnsupportedMediaType)?;
            let path = self.d.store.resolve(&key)?;
            let written = hash_file(&path, declared).await?;
            self.finish(scope, &media, &key, kind, written).await
        }
        .await;
        if outcome.is_err() {
            self.discard(scope, id, &key).await;
        }
        outcome
    }

    /// Sniffs, probes, thumbnails, and marks the asset ready. The caller discards on error.
    async fn finish(
        &self,
        scope: &OwnerScope,
        media: &Media,
        key: &StorageKey,
        claimed: MediaKind,
        written: Written,
    ) -> Result<Media, MediaServiceError> {
        let sniffed = sniff(&written.head).ok_or(MediaServiceError::UnsupportedMediaType)?;
        if sniffed.kind != claimed {
            return Err(MediaServiceError::UnsupportedMediaType);
        }
        let path = self.d.store.resolve(key)?;
        let info = self
            .d
            .probe
            .probe(&path, sniffed.mime)
            .await
            .map_err(|_| MediaServiceError::UnsupportedMediaType)?;
        let thumb_key = key.thumbnail();
        let thumb_path = self.d.store.resolve(&thumb_key)?;
        let made = self
            .d
            .thumbnailer
            .make(&path, sniffed.kind, &info, &thumb_path)
            .await;
        let ready = ReadyMedia {
            mime: sniffed.mime.to_string(),
            bytes: to_i64(written.bytes)?,
            sha256: written.sha256.unwrap_or_default(),
            width: dimension(info, |i| i.width),
            height: dimension(info, |i| i.height),
            duration_ms: info.duration_ms.and_then(|d| i64::try_from(d).ok()),
            thumbnail_key: made.then(|| thumb_key.as_str().to_string()),
        };
        let mut conn = self.d.pool.acquire().await?;
        Ok(MediaRepo::set_ready(&mut conn, scope, media.id, ready).await?)
    }
}

fn dimension(info: ProbeInfo, pick: impl Fn(&ProbeInfo) -> Option<u32>) -> Option<i32> {
    pick(&info).and_then(|v| i32::try_from(v).ok())
}

/// Cuts the file to `declared` bytes (if longer), then hashes it in one streaming pass and
/// captures the head the sniffer needs.
async fn hash_file(path: &Path, declared: u64) -> Result<Written, MediaServiceError> {
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .await?;
    let len = file.metadata().await?.len();
    if len < declared {
        return Err(MediaServiceError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "stored upload is shorter than declared",
        )));
    }
    if len > declared {
        file.set_len(declared).await?;
    }
    let mut reader = tokio::io::BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut head = Vec::with_capacity(SNIFF_LEN);
    let mut buf = vec![0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if head.len() < SNIFF_LEN {
            let take = (SNIFF_LEN - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        hasher.update(&buf[..n]);
        bytes += n as u64;
    }
    Ok(Written {
        bytes,
        sha256: Some(hasher.finalize().to_vec()),
        head,
    })
}

/// Storage use against the quota. A quota of `0` means unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Bytes stored or reserved by open uploads.
    pub used_bytes: u64,
    /// The owner's quota in bytes.
    pub quota_bytes: u64,
}

/// A stored file ready to be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedFile {
    /// Where the bytes are.
    pub path: PathBuf,
    /// The MIME type to serve them as.
    pub mime: String,
}

/// Why a public URL did not serve a file. The two answers are all a caller learns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicMediaError {
    /// The signature is wrong, missing, or expired.
    Forbidden,
    /// The signature is good but the asset is gone or not ready.
    NotFound,
}

/// Short-lived signed URLs for showing an asset in the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewUrls {
    /// The file.
    pub content_url: Url,
    /// Its thumbnail, when it has one.
    pub thumbnail_url: Option<Url>,
    /// When both stop working.
    pub expires_at: DateTime<Utc>,
}

fn core_asset(owner: UserId, media: &Media) -> MediaAsset {
    let state = match media.status {
        MediaStatus::Uploading | MediaStatus::Processing => MediaAssetState::Uploading,
        MediaStatus::Ready => MediaAssetState::Ready,
        MediaStatus::Failed => MediaAssetState::Failed,
    };
    MediaAsset {
        id: media.id,
        owner_id: owner,
        kind: if media.mime.starts_with("image/") {
            MediaKind::Image
        } else {
            MediaKind::Video
        },
        mime: media.mime.clone(),
        bytes: u64::try_from(media.bytes).unwrap_or(0),
        width: media.width.and_then(|v| u32::try_from(v).ok()),
        height: media.height.and_then(|v| u32::try_from(v).ok()),
        duration_ms: media.duration_ms.and_then(|v| u64::try_from(v).ok()),
        sha256: media.sha256.as_deref().map(hex::encode),
        state,
        derived_from: media.derived_from,
    }
}

impl MediaService {
    /// The owner's assets, newest first.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::Scope`] without `view` access.
    pub async fn list(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<Media>, MediaServiceError> {
        let mut conn = self.d.pool.acquire().await?;
        Ok(MediaRepo::list(&mut conn, scope, pagination).await?)
    }

    /// One asset.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when the owner has no such asset.
    pub async fn get(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<Media, MediaServiceError> {
        let mut conn = self.d.pool.acquire().await?;
        Ok(MediaRepo::get(&mut conn, scope, id).await?)
    }

    /// Storage used against the quota.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::Scope`] without `view` access.
    pub async fn usage(&self, scope: &OwnerScope) -> Result<Usage, MediaServiceError> {
        scope
            .require(Capability::View)
            .map_err(MediaServiceError::Scope)?;
        let mut conn = self.d.pool.acquire().await?;
        let used = MediaRepo::stored_bytes(&mut conn, scope.owner).await?;
        let quota = match UserLimitsRepo::get(&mut conn, scope.owner).await {
            Ok(limits) => limits.storage_quota_bytes,
            Err(DataError::NotFound) => self.d.settings.user_quota_bytes,
            Err(err) => return Err(err.into()),
        };
        Ok(Usage {
            used_bytes: u64::try_from(used).unwrap_or(0),
            quota_bytes: quota,
        })
    }

    /// A ready asset's file.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when the asset is missing or not ready.
    pub async fn content(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<ServedFile, MediaServiceError> {
        let media = self.get(scope, id).await?;
        self.served(&media, false)
    }

    /// A ready asset's JPEG thumbnail.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when the asset is missing, not ready, or has none.
    pub async fn thumbnail(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<ServedFile, MediaServiceError> {
        let media = self.get(scope, id).await?;
        self.served(&media, true)
    }

    fn served(&self, media: &Media, thumbnail: bool) -> Result<ServedFile, MediaServiceError> {
        if media.status != MediaStatus::Ready {
            return Err(MediaServiceError::NotFound);
        }
        let (key, mime) = if thumbnail {
            let key = media
                .thumbnail_key
                .as_deref()
                .ok_or(MediaServiceError::NotFound)?;
            (key, "image/jpeg".to_string())
        } else {
            (media.storage_key.as_str(), media.mime.clone())
        };
        let key = StorageKey::parse(key).ok_or(MediaServiceError::NotFound)?;
        Ok(ServedFile {
            path: self.d.store.resolve(&key)?,
            mime,
        })
    }

    /// The file a signed public URL names. The signature is checked before any query, so a
    /// forged URL gets the same answer whether or not the asset exists.
    ///
    /// # Errors
    ///
    /// [`PublicMediaError::Forbidden`] for a bad, missing, or expired signature, and
    /// [`PublicMediaError::NotFound`] when the asset is gone or not ready.
    pub async fn public(
        &self,
        target: &SignedTarget,
        exp: i64,
        sig: &str,
    ) -> Result<ServedFile, PublicMediaError> {
        if !self.d.signer.verify(target, exp, sig, self.d.clock.now()) {
            return Err(PublicMediaError::Forbidden);
        }
        let (SignedTarget::Content(id) | SignedTarget::Thumbnail(id)) = *target;
        let mut conn = self
            .d
            .pool
            .acquire()
            .await
            .map_err(|_| PublicMediaError::NotFound)?;
        let (_owner, media) = MediaRepo::find_ready_public(&mut conn, id)
            .await
            .map_err(|_| PublicMediaError::NotFound)?
            .ok_or(PublicMediaError::NotFound)?;
        self.served(&media, matches!(target, SignedTarget::Thumbnail(_)))
            .map_err(|_| PublicMediaError::NotFound)
    }

    /// Short-lived signed URLs for showing a ready asset in the app.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when the asset is missing or not ready.
    pub async fn view_urls(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<ViewUrls, MediaServiceError> {
        let media = self.get(scope, id).await?;
        if media.status != MediaStatus::Ready {
            return Err(MediaServiceError::NotFound);
        }
        let ttl = chrono::Duration::from_std(self.d.settings.view_url_ttl)
            .map_err(|_| invalid("view_url_ttl"))?;
        let expires_at = self.d.clock.now() + ttl;
        Ok(ViewUrls {
            content_url: self.d.signer.sign(&SignedTarget::Content(id), expires_at),
            thumbnail_url: media
                .thumbnail_key
                .is_some()
                .then(|| self.d.signer.sign(&SignedTarget::Thumbnail(id), expires_at)),
            expires_at,
        })
    }

    /// Deletes an asset and its files. A file that cannot be removed is logged with its key and
    /// left for the garbage collector.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::InUse`] while a post or an unfinished job uses the asset.
    pub async fn delete(
        &self,
        scope: &OwnerScope,
        id: MediaAssetId,
    ) -> Result<(), MediaServiceError> {
        let mut conn = self.d.pool.acquire().await?;
        let gone = MediaRepo::delete(&mut conn, scope, id).await?;
        drop(conn);
        let keys = std::iter::once(gone.storage_key).chain(gone.thumbnail_key);
        for key in keys {
            let Some(parsed) = StorageKey::parse(&key) else {
                continue;
            };
            let path = self.d.store.resolve(&parsed)?;
            if let Err(err) = tokio::fs::remove_file(&path).await
                && err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(key = %key, error = %err, "could not remove deleted media");
            }
        }
        Ok(())
    }

    /// The core view of the owner's assets, for agents. A processing upload reads as uploading.
    ///
    /// # Errors
    ///
    /// [`MediaServiceError::NotFound`] when an ID is missing or belongs to another owner.
    pub async fn assets(
        &self,
        scope: &OwnerScope,
        ids: &[MediaAssetId],
    ) -> Result<Vec<MediaAsset>, MediaServiceError> {
        let mut conn = self.d.pool.acquire().await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let media = MediaRepo::get(&mut conn, scope, *id).await?;
            out.push(core_asset(scope.owner, &media));
        }
        Ok(out)
    }
}
