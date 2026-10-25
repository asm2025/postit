//! Storage keys and the file store.
//!
//! Paths are built only from generated keys, never from anything a user typed, and every
//! resolved path is checked to stay under the storage root.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use futures::StreamExt;
use postit_core::{ByteStream, MediaAsset, MediaAssetId, MediaError, MediaStore, UserId};
use sqlx::PgPool;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

const THUMB_SUFFIX: &str = ".thumb.jpg";

/// A generated storage key: `{owner_uuid}/{yyyy}/{mm}/{asset_uuid}`, plus `.thumb.jpg` for a
/// thumbnail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageKey(String);

fn is_canonical_uuid(text: &str) -> bool {
    Uuid::parse_str(text).is_ok_and(|id| id.hyphenated().to_string() == text)
}

impl StorageKey {
    /// The key of the asset `id` of `owner`, created at `at` (UTC month).
    #[must_use]
    pub fn new(owner: UserId, id: MediaAssetId, at: DateTime<Utc>) -> Self {
        Self(format!(
            "{}/{:04}/{:02}/{}",
            owner.as_uuid().hyphenated(),
            at.year(),
            at.month(),
            id.as_uuid().hyphenated(),
        ))
    }

    /// Accepts only the key grammar: lowercase hyphenated UUIDs, a 4-digit year, a 2-digit
    /// month, and an optional `.thumb.jpg`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('/');
        let (owner, year, month, file) =
            (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let asset = file.strip_suffix(THUMB_SUFFIX).unwrap_or(file);
        let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
        let month_ok = digits(month, 2) && ("01"..="12").contains(&month);
        (is_canonical_uuid(owner) && digits(year, 4) && month_ok && is_canonical_uuid(asset))
            .then(|| Self(text.to_string()))
    }

    /// The key of this asset's thumbnail.
    #[must_use]
    pub fn thumbnail(&self) -> Self {
        if self.0.ends_with(THUMB_SUFFIX) {
            self.clone()
        } else {
            Self(format!("{}{THUMB_SUFFIX}", self.0))
        }
    }

    /// The key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Joins `key` to `root` and checks the result stays under it, including through symlinks in
/// the part of the path that exists.
fn resolve_under(root: &Path, key: &StorageKey) -> std::io::Result<PathBuf> {
    let escape = || {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "path escapes the media root",
        )
    };
    let relative = Path::new(key.as_str());
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(escape());
    }
    let path = root.join(relative);
    if let Some(parent) = path.parent()
        && let Ok(real) = std::fs::canonicalize(parent)
        && !real.starts_with(root)
    {
        return Err(escape());
    }
    Ok(path)
}

/// Media on a local directory.
#[derive(Debug, Clone)]
pub struct FsMediaStore {
    root: PathBuf,
    pool: PgPool,
}

impl FsMediaStore {
    /// Opens the store at `root`, creating the directory when it is missing.
    ///
    /// # Errors
    ///
    /// The I/O error when the root cannot be created or resolved.
    pub fn open_root(root: &Path, pool: PgPool) -> std::io::Result<Self> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: std::fs::canonicalize(root)?,
            pool,
        })
    }

    /// The canonical storage root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path of `key` under the root.
    ///
    /// # Errors
    ///
    /// A permission error when the path would leave the root.
    pub fn resolve(&self, key: &StorageKey) -> std::io::Result<PathBuf> {
        resolve_under(&self.root, key)
    }
}

fn io_error(err: &impl std::fmt::Display) -> MediaError {
    MediaError::Io(err.to_string())
}

#[async_trait]
impl MediaStore for FsMediaStore {
    async fn open(&self, asset: &MediaAsset) -> Result<ByteStream, MediaError> {
        let mut conn = self.pool.acquire().await.map_err(|e| io_error(&e))?;
        let row = postit_data::media::MediaRepo::find_ready(&mut conn, asset.owner_id, asset.id)
            .await
            .map_err(|e| io_error(&e))?
            .ok_or(MediaError::NotFound)?;
        drop(conn);
        let key = StorageKey::parse(&row.storage_key).ok_or(MediaError::NotFound)?;
        let path = self.resolve(&key).map_err(|e| io_error(&e))?;
        let file = tokio::fs::File::open(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                MediaError::NotFound
            } else {
                io_error(&e)
            }
        })?;
        Ok(ReaderStream::new(file).boxed())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{TimeZone, Utc};
    use postit_core::{MediaAssetId, UserId};
    use uuid::Uuid;

    use super::*;

    fn key() -> StorageKey {
        StorageKey::new(
            UserId::from_uuid(Uuid::now_v7()),
            MediaAssetId::from_uuid(Uuid::now_v7()),
            Utc.with_ymd_and_hms(2026, 3, 9, 1, 2, 3).unwrap(),
        )
    }

    #[test]
    fn storage_key_round_trips() {
        let k = key();
        assert!(k.as_str().contains("/2026/03/"));
        assert_eq!(StorageKey::parse(k.as_str()), Some(k.clone()));
        let t = k.thumbnail();
        assert!(t.as_str().ends_with(".thumb.jpg"));
        assert_eq!(StorageKey::parse(t.as_str()), Some(t));
    }

    #[test]
    fn storage_key_parse_refuses_traversal() {
        let k = key();
        let good = k.as_str().to_string();
        let upper = good.to_uppercase();
        for bad in [
            "../x".to_string(),
            "/etc/passwd".to_string(),
            r"C:\x".to_string(),
            good.replace('/', "\\"),
            format!("{good}/extra"),
            format!("../{good}"),
            format!("{good}.thumb.jpg.exe"),
            format!("{good}.png"),
            upper,
            good.replace("/03/", "/13/"),
            good.replace("/2026/", "/26/"),
            String::new(),
        ] {
            assert_eq!(StorageKey::parse(&bad), None, "{bad}");
        }
    }

    #[test]
    fn resolve_stays_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let k = key();
        let path = resolve_under(&root, &k).unwrap();
        assert!(path.starts_with(&root));
        assert!(path.ends_with(k.as_str()));
    }
}
