//! Helpers for tests that need a working media service.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use postit_config::{MediaSettings, RedactedSecret};
use postit_core::{Clock, MediaProbe, SystemIdGenerator};
use sqlx::PgPool;

use crate::{FsMediaStore, MediaService, MediaServiceDeps, NativeProbe, Thumbnailer, UrlSigner};

/// Media settings with the shipped defaults, storing under `root`, signing with a fixed test
/// key.
#[must_use]
pub fn media_settings(root: &Path) -> MediaSettings {
    MediaSettings {
        storage_root: root.to_path_buf(),
        signing_key: Some(RedactedSecret::from(
            "media-test-key-0123456789abcdef0123456789".to_string(),
        )),
        public_base_url: None,
        single_request_max: 20_000_000,
        chunk_max: 16_000_000,
        upload_session_ttl: Duration::from_hours(24),
        view_url_ttl: Duration::from_mins(10),
        public_url_ttl: Duration::from_hours(1),
        user_quota_bytes: 21_474_836_480,
        fit_concurrency: 1,
        fit_max_duration: Duration::from_mins(10),
        ffmpeg_path: "ffmpeg".into(),
        image_max_bytes: 52_428_800,
        video_max_bytes: 4_294_967_296,
        thumbnail_max_pixels: 40_000_000,
        ffprobe_path: "ffprobe".into(),
        idle_timeout: Duration::from_secs(60),
        complete_timeout: Duration::from_mins(10),
    }
}

/// A service over `pool` storing under `settings.storage_root`, reading time from `clock`.
///
/// # Errors
///
/// The I/O error when the storage root cannot be created.
pub fn media_service(
    pool: PgPool,
    settings: MediaSettings,
    clock: Arc<dyn Clock>,
) -> std::io::Result<MediaService> {
    media_service_with_probe(pool, settings, clock, Arc::new(NativeProbe))
}

/// [`media_service`] with its own probe, for tests that need a slow or failing one.
///
/// # Errors
///
/// The I/O error when the storage root cannot be created.
pub fn media_service_with_probe(
    pool: PgPool,
    settings: MediaSettings,
    clock: Arc<dyn Clock>,
    probe: Arc<dyn MediaProbe>,
) -> std::io::Result<MediaService> {
    let store = Arc::new(FsMediaStore::open_root(
        &settings.storage_root,
        pool.clone(),
    )?);
    let base = "https://media.test/"
        .parse()
        .unwrap_or_else(|_| unreachable!("constant URL"));
    Ok(MediaService::new(MediaServiceDeps {
        pool,
        store,
        probe,
        thumbnailer: Thumbnailer::new(settings.thumbnail_max_pixels, settings.ffmpeg_path.clone()),
        signer: Arc::new(UrlSigner::new(
            b"media-test-signing-key-0123456789abcdef",
            base,
        )),
        clock,
        ids: Arc::new(SystemIdGenerator),
        settings,
    }))
}
