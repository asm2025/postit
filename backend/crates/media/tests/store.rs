#![allow(clippy::unwrap_used)]

use chrono::{Duration, Utc};
use futures::StreamExt;
use postit_core::{
    MediaAsset, MediaAssetId, MediaAssetState, MediaError, MediaKind, MediaStore, UserId,
};
use postit_data::media::{MediaRepo, MediaStatus, NewMedia};
use postit_data::testkit::{callers, seed_user};
use postit_media::{FsMediaStore, StorageKey};
use sqlx::PgPool;
use uuid::Uuid;

async fn seed(pool: &PgPool, owner: UserId, status: MediaStatus, key: &StorageKey) -> MediaAsset {
    let who = callers(pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let id = MediaAssetId::from_uuid(Uuid::now_v7());
    MediaRepo::insert(
        &mut c,
        &who.owner,
        NewMedia {
            id,
            storage_key: key.as_str().to_string(),
            original_filename: None,
            mime: "image/png".into(),
            bytes: 5,
            status,
            declared_bytes: None,
            upload_expires_at: Some(Utc::now() + Duration::hours(1)),
            derived_from: None,
            fit_spec: None,
        },
    )
    .await
    .unwrap();
    MediaAsset {
        id,
        owner_id: owner,
        kind: MediaKind::Image,
        mime: "image/png".into(),
        bytes: 5,
        width: None,
        height: None,
        duration_ms: None,
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

fn store(pool: &PgPool) -> (tempfile::TempDir, FsMediaStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsMediaStore::open_root(dir.path(), pool.clone()).unwrap();
    (dir, store)
}

fn fresh_key(owner: UserId) -> StorageKey {
    StorageKey::new(owner, MediaAssetId::from_uuid(Uuid::now_v7()), Utc::now())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn open_streams_a_ready_asset(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let (_dir, store) = store(&pool);
    let key = fresh_key(owner);
    let asset = seed(&pool, owner, MediaStatus::Ready, &key).await;
    let path = store.resolve(&key).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"hello").unwrap();

    let mut stream = store.open(&asset).await.unwrap();
    let mut got = Vec::new();
    while let Some(chunk) = stream.next().await {
        got.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(got, b"hello");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn open_refuses_uploading_and_foreign_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let (_dir, store) = store(&pool);
    let uploading = seed(&pool, owner, MediaStatus::Uploading, &fresh_key(owner)).await;
    assert!(matches!(
        store.open(&uploading).await,
        Err(MediaError::NotFound)
    ));

    let ready = seed(&pool, owner, MediaStatus::Ready, &fresh_key(owner)).await;
    let foreign = MediaAsset {
        owner_id: other,
        ..ready
    };
    assert!(matches!(
        store.open(&foreign).await,
        Err(MediaError::NotFound)
    ));
}
