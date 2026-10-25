#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use postit_core::{ByteStream, MediaAssetState, MediaKind, SystemClock, UserId};
use postit_data::OwnerScope;
use postit_data::media::{Media, MediaStatus};
use postit_data::testkit::seed_user;
use postit_media::testkit::{media_service, media_settings};
use postit_media::{MediaService, MediaServiceError, PublicMediaError, SignedTarget};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

fn whole(bytes: &[u8]) -> ByteStream {
    futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes))]).boxed()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

struct Env {
    dir: tempfile::TempDir,
    owner: UserId,
    scope: OwnerScope,
    service: MediaService,
}

async fn env(pool: PgPool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let owner = seed_user(&pool, "active").await;
    let service = media_service(
        pool.clone(),
        media_settings(dir.path()),
        Arc::new(SystemClock),
    )
    .unwrap();
    Env {
        scope: OwnerScope::own(owner),
        dir,
        owner,
        service,
    }
}

async fn upload_png(env: &Env, name: &str) -> (Media, Vec<u8>) {
    let bytes = png(40, 20);
    let media = env
        .service
        .upload(&env.scope, name, bytes.len() as u64, whole(&bytes))
        .await
        .unwrap();
    (media, bytes)
}

fn query(url: &url::Url, key: &str) -> String {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn usage_counts_ready_and_open_sessions(pool: PgPool) {
    let env = env(pool.clone()).await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 1, 1, 5000)",
    )
    .bind(env.owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let (_media, bytes) = upload_png(&env, "a.png").await;
    env.service
        .start_session(&env.scope, "b.png", 1000)
        .await
        .unwrap();
    let usage = env.service.usage(&env.scope).await.unwrap();
    assert_eq!(usage.used_bytes, bytes.len() as u64 + 1000);
    assert_eq!(usage.quota_bytes, 5000);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn usage_falls_back_to_the_configured_quota(pool: PgPool) {
    let env = env(pool).await;
    let usage = env.service.usage(&env.scope).await.unwrap();
    assert_eq!(usage.used_bytes, 0);
    assert_eq!(usage.quota_bytes, 21_474_836_480);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn view_urls_verify_with_the_public_lookup(pool: PgPool) {
    let env = env(pool).await;
    let (media, bytes) = upload_png(&env, "a.png").await;
    let urls = env.service.view_urls(&env.scope, media.id).await.unwrap();
    let exp: i64 = query(&urls.content_url, "exp").parse().unwrap();
    assert_eq!(exp, urls.expires_at.timestamp());
    let served = env
        .service
        .public(
            &SignedTarget::Content(media.id),
            exp,
            &query(&urls.content_url, "sig"),
        )
        .await
        .unwrap();
    assert_eq!(served.mime, "image/png");
    assert_eq!(std::fs::read(&served.path).unwrap(), bytes);

    let thumb_url = urls.thumbnail_url.unwrap();
    let thumb = env
        .service
        .public(
            &SignedTarget::Thumbnail(media.id),
            query(&thumb_url, "exp").parse().unwrap(),
            &query(&thumb_url, "sig"),
        )
        .await
        .unwrap();
    assert_eq!(thumb.mime, "image/jpeg");
    assert!(thumb.path.to_string_lossy().ends_with(".thumb.jpg"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_refuses_bad_signature_before_lookup(pool: PgPool) {
    let env = env(pool).await;
    let (media, _) = upload_png(&env, "a.png").await;
    let exp = (Utc::now() + chrono::Duration::minutes(5)).timestamp();
    let missing = postit_core::MediaAssetId::from_uuid(Uuid::now_v7());
    let forged_missing = env
        .service
        .public(&SignedTarget::Content(missing), exp, "AAAA")
        .await;
    let forged_existing = env
        .service
        .public(&SignedTarget::Content(media.id), exp, "AAAA")
        .await;
    assert!(matches!(forged_missing, Err(PublicMediaError::Forbidden)));
    assert!(matches!(forged_existing, Err(PublicMediaError::Forbidden)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_not_found_when_deleted(pool: PgPool) {
    let env = env(pool).await;
    let (media, _) = upload_png(&env, "a.png").await;
    let urls = env.service.view_urls(&env.scope, media.id).await.unwrap();
    env.service.delete(&env.scope, media.id).await.unwrap();
    let got = env
        .service
        .public(
            &SignedTarget::Content(media.id),
            query(&urls.content_url, "exp").parse().unwrap(),
            &query(&urls.content_url, "sig"),
        )
        .await;
    assert!(matches!(got, Err(PublicMediaError::NotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn content_refuses_not_ready(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 100)
        .await
        .unwrap();
    assert!(matches!(
        env.service.content(&env.scope, session.id).await,
        Err(MediaServiceError::NotFound)
    ));
    assert!(matches!(
        env.service.thumbnail(&env.scope, session.id).await,
        Err(MediaServiceError::NotFound)
    ));
    assert!(matches!(
        env.service.view_urls(&env.scope, session.id).await,
        Err(MediaServiceError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn content_and_thumbnail_serve_a_ready_asset(pool: PgPool) {
    let env = env(pool).await;
    let (media, bytes) = upload_png(&env, "a.png").await;
    let content = env.service.content(&env.scope, media.id).await.unwrap();
    assert_eq!(content.mime, "image/png");
    assert_eq!(std::fs::read(&content.path).unwrap(), bytes);
    let thumb = env.service.thumbnail(&env.scope, media.id).await.unwrap();
    assert_eq!(thumb.mime, "image/jpeg");
    assert!(thumb.path.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_removes_files(pool: PgPool) {
    let env = env(pool).await;
    let (media, _) = upload_png(&env, "a.png").await;
    let file = env.dir.path().join(&media.storage_key);
    let thumb = env.dir.path().join(media.thumbnail_key.as_ref().unwrap());
    assert!(file.exists() && thumb.exists());
    env.service.delete(&env.scope, media.id).await.unwrap();
    assert!(!file.exists() && !thumb.exists());
    assert!(matches!(
        env.service.get(&env.scope, media.id).await,
        Err(MediaServiceError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_in_use_keeps_files(pool: PgPool) {
    let env = env(pool.clone()).await;
    let (media, _) = upload_png(&env, "a.png").await;
    let post = Uuid::now_v7();
    sqlx::query("INSERT INTO posts (id, owner_id) VALUES ($1, $2)")
        .bind(post)
        .bind(env.owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO post_media (post_id, media_id, owner_id, position) VALUES ($1, $2, $3, 0)",
    )
    .bind(post)
    .bind(media.id.as_uuid())
    .bind(env.owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let err = env.service.delete(&env.scope, media.id).await.unwrap_err();
    assert!(matches!(err, MediaServiceError::InUse), "{err:?}");
    assert!(env.dir.path().join(&media.storage_key).exists());
    env.service.get(&env.scope, media.id).await.unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assets_maps_processing_to_uploading_and_hex_sha(pool: PgPool) {
    let env = env(pool.clone()).await;
    let (ready, bytes) = upload_png(&env, "a.png").await;
    let session = env
        .service
        .start_session(&env.scope, "b.mp4", 100)
        .await
        .unwrap();
    sqlx::query("UPDATE media_assets SET status = 'processing' WHERE id = $1")
        .bind(session.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let got = env
        .service
        .assets(&env.scope, &[ready.id, session.id])
        .await
        .unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].id, ready.id);
    assert_eq!(got[0].owner_id, env.owner);
    assert_eq!(got[0].kind, MediaKind::Image);
    assert_eq!(got[0].state, MediaAssetState::Ready);
    assert_eq!(
        got[0].sha256.as_deref(),
        Some(hex::encode(Sha256::digest(&bytes)).as_str())
    );
    assert_eq!((got[0].width, got[0].height), (Some(40), Some(20)));
    assert_eq!(got[0].bytes, bytes.len() as u64);
    assert_eq!(got[1].kind, MediaKind::Video);
    assert_eq!(got[1].state, MediaAssetState::Uploading);
    assert_eq!(MediaStatus::Processing.as_str(), "processing");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assets_refuses_foreign_id(pool: PgPool) {
    let env = env(pool.clone()).await;
    let (media, _) = upload_png(&env, "a.png").await;
    let other = seed_user(&pool, "active").await;
    let err = env
        .service
        .assets(&OwnerScope::own(other), &[media.id])
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_pages_newest_first(pool: PgPool) {
    let env = env(pool).await;
    let (a, _) = upload_png(&env, "a.png").await;
    let (b, _) = upload_png(&env, "b.png").await;
    let page = env
        .service
        .list(
            &env.scope,
            emixdb::dto::Pagination {
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.data[0].id, b.id);
    assert_eq!(page.data[1].id, a.id);
}
