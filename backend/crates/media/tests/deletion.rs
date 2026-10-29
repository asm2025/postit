//! The media step of user deletion (Task 13): it removes the user's whole key tree.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use postit_core::{MediaAssetId, UserId};
use postit_data::OwnerScope;
use postit_data::media::{MediaRepo, MediaStatus, NewMedia};
use postit_data::testkit::seed_user;
use postit_identity::deletion::UserDeletionStep;
use postit_media::{FsMediaStore, MediaDeletionStep, StorageKey};
use sqlx::PgPool;
use uuid::Uuid;

fn write(store: &FsMediaStore, key: &StorageKey) -> PathBuf {
    let path = store.resolve(key).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"bytes").unwrap();
    path
}

fn key(owner: UserId, at: chrono::DateTime<Utc>) -> StorageKey {
    StorageKey::new(owner, MediaAssetId::from_uuid(Uuid::now_v7()), at)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn media_step_removes_files_and_tolerates_missing(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsMediaStore::open_root(dir.path(), pool.clone()).unwrap());
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let now = Utc::now();

    // An original and its thumbnail, in two months.
    let original = key(owner, now);
    let original_path = write(&store, &original);
    let thumb_path = write(&store, &original.thumbnail());
    let older = key(owner, now - Duration::days(62));
    let older_path = write(&store, &older);
    // An unfinished upload: the bytes received so far sit at its key.
    let upload = MediaAssetId::from_uuid(Uuid::now_v7());
    let upload_key = StorageKey::new(owner, upload, now);
    {
        let mut conn = pool.acquire().await.unwrap();
        MediaRepo::reserve(
            &mut conn,
            &OwnerScope::own(owner),
            NewMedia {
                id: upload,
                storage_key: upload_key.as_str().to_owned(),
                original_filename: Some("a.png".into()),
                mime: "application/octet-stream".into(),
                bytes: 0,
                status: MediaStatus::Uploading,
                declared_bytes: Some(10),
                upload_expires_at: Some(now + Duration::hours(1)),
                derived_from: None,
                fit_spec: None,
            },
        )
        .await
        .unwrap();
    }
    let upload_path = write(&store, &upload_key);
    // Another user's file stays.
    let theirs = write(&store, &key(other, now));

    let step = MediaDeletionStep::new(Arc::clone(&store));
    assert_eq!(step.name(), "media");
    step.run(owner).await.unwrap();

    for path in [&original_path, &thumb_path, &older_path, &upload_path] {
        assert!(!path.exists(), "{} still exists", path.display());
    }
    assert!(!store.root().join(owner.as_uuid().to_string()).exists());
    assert!(theirs.exists());

    // A second run finds nothing and succeeds.
    step.run(owner).await.unwrap();
    assert!(theirs.exists());
}
