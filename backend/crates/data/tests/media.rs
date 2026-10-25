#![allow(clippy::unwrap_used)]

use chrono::{Duration, Utc};
use emixdb::dto::Pagination;
use postit_core::MediaAssetId;
use postit_data::media::{MediaRepo, MediaStatus, NewMedia, ReadyMedia};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

fn new_media(status: MediaStatus, bytes: i64, declared: Option<i64>) -> NewMedia {
    NewMedia {
        id: MediaAssetId::from_uuid(Uuid::now_v7()),
        storage_key: format!("media/{}", Uuid::now_v7()),
        original_filename: Some("photo.jpg".to_string()),
        mime: "image/jpeg".to_string(),
        bytes,
        status,
        declared_bytes: declared,
        upload_expires_at: Some(Utc::now() + Duration::hours(1)),
        derived_from: None,
        fit_spec: None,
    }
}

fn ready(bytes: i64) -> ReadyMedia {
    ReadyMedia {
        bytes,
        sha256: vec![7; 32],
        width: Some(640),
        height: Some(480),
        duration_ms: None,
        thumbnail_key: Some("thumbs/1".to_string()),
    }
}

#[sqlx::test]
async fn media_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let asset = MediaRepo::insert(&mut c, &who.edit, new_media(MediaStatus::Ready, 10, None))
        .await
        .unwrap_or_else(|e| unreachable!("edit insert: {e}"));

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            MediaRepo::get(&mut c, scope, asset.id).await.unwrap().id,
            asset.id
        );
        assert_eq!(
            MediaRepo::list(&mut c, scope, page()).await.unwrap().total,
            1
        );
    }
    assert!(matches!(
        MediaRepo::insert(&mut c, &who.view, new_media(MediaStatus::Ready, 1, None)).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::delete(&mut c, &who.view, asset.id).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::get(&mut c, &who.stranger, asset.id).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        MediaRepo::list(&mut c, &who.stranger, page())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(matches!(
        MediaRepo::delete(&mut c, &who.stranger, asset.id).await,
        Err(DataError::NotFound)
    ));
    MediaRepo::delete(&mut c, &who.edit, asset.id)
        .await
        .unwrap();
}

#[sqlx::test]
async fn set_ready_records_the_file_and_closes_the_session(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let asset = MediaRepo::insert(
        &mut c,
        &who.owner,
        new_media(MediaStatus::Uploading, 0, Some(500)),
    )
    .await
    .unwrap();
    assert_eq!(asset.status, MediaStatus::Uploading);

    let done = MediaRepo::set_ready(&mut c, &who.owner, asset.id, ready(480))
        .await
        .unwrap();
    assert_eq!(done.status, MediaStatus::Ready);
    assert_eq!(done.bytes, 480);
    assert_eq!(done.width, Some(640));
    assert_eq!(done.sha256.as_deref(), Some(&[7u8; 32][..]));
    assert!(done.upload_expires_at.is_none());

    assert!(matches!(
        MediaRepo::set_ready(&mut c, &who.owner, asset.id, ready(480)).await,
        Err(DataError::Conflict(reason)) if reason == "media_not_pending"
    ));
    assert!(matches!(
        MediaRepo::set_ready(
            &mut c,
            &who.owner,
            MediaAssetId::from_uuid(Uuid::now_v7()),
            ready(1)
        )
        .await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn media_in_use_cannot_be_deleted(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let asset = MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Ready, 10, None))
        .await
        .unwrap();
    let post = Uuid::now_v7();
    sqlx::query("INSERT INTO posts (id, owner_id) VALUES ($1, $2)")
        .bind(post)
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO post_media (post_id, media_id, owner_id, position) VALUES ($1, $2, $3, 0)",
    )
    .bind(post)
    .bind(asset.id.as_uuid())
    .bind(owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    assert!(matches!(
        MediaRepo::delete(&mut c, &who.owner, asset.id).await,
        Err(DataError::Conflict(reason)) if reason == "media_in_use"
    ));
    sqlx::query("DELETE FROM post_media")
        .execute(&pool)
        .await
        .unwrap();
    MediaRepo::delete(&mut c, &who.owner, asset.id)
        .await
        .unwrap();
}

#[sqlx::test]
async fn stored_bytes_counts_ready_and_open_sessions(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    assert_eq!(MediaRepo::stored_bytes(&mut c, owner).await.unwrap(), 0);

    MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Ready, 100, None))
        .await
        .unwrap();
    MediaRepo::insert(
        &mut c,
        &who.owner,
        new_media(MediaStatus::Uploading, 30, Some(1000)),
    )
    .await
    .unwrap();
    MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Failed, 55, None))
        .await
        .unwrap();
    MediaRepo::insert(
        &mut c,
        &postit_data::OwnerScope::own(other),
        new_media(MediaStatus::Ready, 9999, None),
    )
    .await
    .unwrap();

    assert_eq!(MediaRepo::stored_bytes(&mut c, owner).await.unwrap(), 1100);
}
