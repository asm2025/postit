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

async fn set_quota(pool: &PgPool, owner: postit_core::UserId, quota: i64) {
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 10, 10, $2)
         ON CONFLICT (user_id) DO UPDATE SET storage_quota_bytes = $2",
    )
    .bind(owner.as_uuid())
    .bind(quota)
    .execute(pool)
    .await
    .unwrap();
}

fn upload(declared: i64) -> NewMedia {
    new_media(MediaStatus::Uploading, 0, Some(declared))
}

#[sqlx::test]
async fn reserve_refuses_over_quota(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    set_quota(&pool, owner, 100).await;
    let mut c = pool.acquire().await.unwrap();
    MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Ready, 60, None))
        .await
        .unwrap();
    let err = MediaRepo::reserve(&mut c, &who.owner, upload(50))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            DataError::StorageQuotaExceeded {
                quota: 100,
                used: 60
            }
        ),
        "{err:?}"
    );
    let ok = MediaRepo::reserve(&mut c, &who.owner, upload(40))
        .await
        .unwrap();
    assert_eq!(ok.upload_offset, Some(0));
    assert_eq!(ok.status, MediaStatus::Uploading);
}

#[sqlx::test]
async fn reserve_unlimited_when_quota_zero(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    set_quota(&pool, owner, 0).await;
    let mut c = pool.acquire().await.unwrap();
    MediaRepo::reserve(&mut c, &who.owner, upload(i64::MAX / 4))
        .await
        .unwrap();
    MediaRepo::reserve(&mut c, &who.owner, upload(i64::MAX / 4))
        .await
        .unwrap();
}

#[sqlx::test]
async fn concurrent_reservations_never_exceed_quota(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    set_quota(&pool, owner, 100).await;
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        let scope = who.owner.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = pool.acquire().await.unwrap();
            MediaRepo::reserve(&mut c, &scope, upload(30)).await.is_ok()
        }));
    }
    let mut granted = 0;
    for t in tasks {
        if t.await.unwrap() {
            granted += 1;
        }
    }
    assert_eq!(granted, 3);
    let mut c = pool.acquire().await.unwrap();
    assert!(MediaRepo::stored_bytes(&mut c, owner).await.unwrap() <= 100);
}

#[sqlx::test]
async fn processing_counts_against_quota(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    set_quota(&pool, owner, 100).await;
    let mut c = pool.acquire().await.unwrap();
    let a = MediaRepo::reserve(&mut c, &who.owner, upload(80))
        .await
        .unwrap();
    MediaRepo::set_offset(&mut c, &who.owner, a.id, 80)
        .await
        .unwrap();
    MediaRepo::set_processing(&mut c, &who.owner, a.id)
        .await
        .unwrap();
    assert_eq!(MediaRepo::stored_bytes(&mut c, owner).await.unwrap(), 80);
    assert!(matches!(
        MediaRepo::reserve(&mut c, &who.owner, upload(30)).await,
        Err(DataError::StorageQuotaExceeded { .. })
    ));
}

#[sqlx::test]
async fn lock_session_refuses_expired_and_ready(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let open = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    let now = Utc::now();
    let locked = MediaRepo::lock_session(&mut c, &who.owner, open.id, now)
        .await
        .unwrap();
    assert_eq!(locked.id, open.id);
    assert!(matches!(
        MediaRepo::lock_session(&mut c, &who.owner, open.id, now + Duration::hours(2)).await,
        Err(DataError::NotFound)
    ));
    MediaRepo::set_ready(&mut c, &who.owner, open.id, ready(10))
        .await
        .unwrap();
    assert!(matches!(
        MediaRepo::lock_session(&mut c, &who.owner, open.id, now).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn set_offset_then_lock_reads_it(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let open = MediaRepo::reserve(&mut c, &who.owner, upload(100))
        .await
        .unwrap();
    MediaRepo::set_offset(&mut c, &who.owner, open.id, 40)
        .await
        .unwrap();
    let locked = MediaRepo::lock_session(&mut c, &who.owner, open.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(locked.upload_offset, Some(40));
    assert_eq!(locked.bytes, 40);
}

#[sqlx::test]
async fn set_processing_and_failed_change_state(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let a = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    let p = MediaRepo::set_processing(&mut c, &who.owner, a.id)
        .await
        .unwrap();
    assert_eq!(p.status, MediaStatus::Processing);
    assert!(matches!(
        MediaRepo::set_processing(&mut c, &who.owner, a.id).await,
        Err(DataError::Conflict(r)) if r == "media_not_pending"
    ));
    MediaRepo::set_failed(&mut c, &who.owner, a.id)
        .await
        .unwrap();
    assert_eq!(
        MediaRepo::get(&mut c, &who.owner, a.id)
            .await
            .unwrap()
            .status,
        MediaStatus::Failed
    );
}

#[sqlx::test]
async fn new_methods_follow_the_access_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    for ok in [&who.owner, &who.edit, &who.publish, &who.limited] {
        let a = MediaRepo::reserve(&mut c, ok, upload(10)).await.unwrap();
        MediaRepo::lock_session(&mut c, ok, a.id, Utc::now())
            .await
            .unwrap();
        MediaRepo::set_offset(&mut c, ok, a.id, 1).await.unwrap();
        MediaRepo::set_processing(&mut c, ok, a.id).await.unwrap();
        MediaRepo::set_failed(&mut c, ok, a.id).await.unwrap();
    }
    let a = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    assert!(matches!(
        MediaRepo::reserve(&mut c, &who.view, upload(1)).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::lock_session(&mut c, &who.view, a.id, Utc::now()).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::set_offset(&mut c, &who.view, a.id, 1).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::set_processing(&mut c, &who.view, a.id).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::set_failed(&mut c, &who.view, a.id).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        MediaRepo::lock_session(&mut c, &who.stranger, a.id, Utc::now()).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        MediaRepo::set_offset(&mut c, &who.stranger, a.id, 1).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        MediaRepo::set_processing(&mut c, &who.stranger, a.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        MediaRepo::set_failed(&mut c, &who.stranger, a.id).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn delete_returns_keys(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let a = MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Ready, 10, None))
        .await
        .unwrap();
    let gone = MediaRepo::delete(&mut c, &who.owner, a.id).await.unwrap();
    assert_eq!(gone.storage_key, a.storage_key);
    assert_eq!(gone.id, a.id);
}

#[sqlx::test]
async fn find_ready_public_ignores_uploading(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let open = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    assert!(
        MediaRepo::find_ready(&mut c, owner, open.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        MediaRepo::find_ready_public(&mut c, open.id)
            .await
            .unwrap()
            .is_none()
    );
    MediaRepo::set_ready(&mut c, &who.owner, open.id, ready(10))
        .await
        .unwrap();
    assert!(
        MediaRepo::find_ready(&mut c, owner, open.id)
            .await
            .unwrap()
            .is_some()
    );
    let (found_owner, m) = MediaRepo::find_ready_public(&mut c, open.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found_owner, owner);
    assert_eq!(m.id, open.id);
    let other = seed_user(&pool, "active").await;
    assert!(
        MediaRepo::find_ready(&mut c, other, open.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn take_garbage_takes_failed_expired_and_stale_processing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let failed = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    MediaRepo::set_failed(&mut c, &who.owner, failed.id)
        .await
        .unwrap();
    let expired = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media_assets SET upload_expires_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(expired.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let stale = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    MediaRepo::set_processing(&mut c, &who.owner, stale.id)
        .await
        .unwrap();
    sqlx::query("UPDATE media_assets SET updated_at = now() - interval '2 days' WHERE id = $1")
        .bind(stale.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let fresh = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    let fresh_processing = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    MediaRepo::set_processing(&mut c, &who.owner, fresh_processing.id)
        .await
        .unwrap();
    let kept_ready = MediaRepo::insert(&mut c, &who.owner, new_media(MediaStatus::Ready, 1, None))
        .await
        .unwrap();

    let now = Utc::now();
    let taken = MediaRepo::take_garbage(&mut c, now, now - Duration::hours(24), 500)
        .await
        .unwrap();
    let mut ids: Vec<_> = taken.iter().map(|g| g.id.as_uuid()).collect();
    ids.sort();
    let mut want = vec![
        failed.id.as_uuid(),
        expired.id.as_uuid(),
        stale.id.as_uuid(),
    ];
    want.sort();
    assert_eq!(ids, want);
    for id in [fresh.id, fresh_processing.id, kept_ready.id] {
        MediaRepo::get(&mut c, &who.owner, id).await.unwrap();
    }
    let again = MediaRepo::take_garbage(&mut c, now, now - Duration::hours(24), 500)
        .await
        .unwrap();
    assert!(again.is_empty());
}

#[sqlx::test]
async fn known_keys_matches_thumbnails(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let a = MediaRepo::reserve(&mut c, &who.owner, upload(10))
        .await
        .unwrap();
    MediaRepo::set_ready(&mut c, &who.owner, a.id, ready(10))
        .await
        .unwrap();
    let keys = vec![
        a.storage_key.clone(),
        "thumbs/1".to_string(),
        "unknown".to_string(),
    ];
    let known = MediaRepo::known_keys(&mut c, &keys).await.unwrap();
    assert!(known.contains(&a.storage_key));
    assert!(known.contains("thumbs/1"));
    assert!(!known.contains("unknown"));
}
