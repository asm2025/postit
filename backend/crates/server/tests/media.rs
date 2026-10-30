//! The media service as the composition root builds it.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use bytes::Bytes;
use futures::StreamExt;
use postit_config::{Environment, Settings, load_with};
use postit_core::{MediaAsset, MediaAssetState, MediaHost, MediaKind, MediaStore};
use postit_data::OwnerScope;
use postit_data::testkit::seed_user;
use postit_server::compose;
use sqlx::PgPool;

fn settings(root: &Path) -> Settings {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "media-test-key"),
        (
            "POSTIT__IDENTITY_MAIL__SIGNING_KEY",
            "identity-mail-test-key-0123456789",
        ),
        (
            "POSTIT__PUSH__VAPID_PRIVATE_KEY",
            "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain([(
        "POSTIT__MEDIA__STORAGE_ROOT".to_string(),
        root.display().to_string(),
    )]);
    load_with(Environment::Development, &dir, vars).unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn startup_refuses_missing_media_signing_key(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = settings(dir.path());
    settings.media.signing_key = None;
    let err = compose::media_service(
        &settings,
        &pool,
        &compose::system_ids(),
        &compose::system_clock(),
    )
    .err()
    .unwrap();
    assert!(format!("{err:#}").contains("media.signing_key"), "{err:#}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn startup_refuses_unwritable_storage_root(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    // A file where the directory should be: the root can be neither created nor written.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let settings = settings(&blocker.join("media"));
    let err = compose::media_service(
        &settings,
        &pool,
        &compose::system_ids(),
        &compose::system_clock(),
    )
    .err()
    .unwrap();
    assert!(format!("{err:#}").contains("media.storage_root"), "{err:#}");
}

#[test]
fn console_lists_media_gc_and_the_oauth_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = compose::console_catalog(&settings(dir.path()));
    assert!(catalog.get("media_gc").is_some());
    assert!(catalog.get("refresh_credentials").is_some());
    assert!(catalog.get("oauth_state_purge").is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn account_service_gets_real_media(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let settings = settings(dir.path());
    let media = compose::media_service(
        &settings,
        &pool,
        &compose::system_ids(),
        &compose::system_clock(),
    )
    .unwrap();
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let png = {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let body = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(png.clone()))]);
    let uploaded = media
        .service
        .upload(&scope, "a.png", png.len() as u64, body.boxed())
        .await
        .unwrap();

    // What agents get: the same store and host the accounts service was built with.
    let assets = media.service.assets(&scope, &[uploaded.id]).await.unwrap();
    let asset: &MediaAsset = &assets[0];
    assert_eq!(asset.kind, MediaKind::Image);
    assert_eq!(asset.state, MediaAssetState::Ready);
    let url = media
        .host
        .public_url(asset, std::time::Duration::from_secs(60))
        .unwrap();
    assert!(url.path().starts_with("/public/media/"), "{url}");
    let mut stream = media.store.open(asset).await.unwrap();
    let mut got = Vec::new();
    while let Some(chunk) = stream.next().await {
        got.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(got, png);
}
