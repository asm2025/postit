#![allow(clippy::unwrap_used)]

use std::time::Duration;

use emixdb::dto::Pagination;
use postit_core::{AccountId, CampaignId, MediaAssetId, PostId, PostKind, UserId};
use postit_data::media::{MediaRepo, MediaStatus, NewMedia};
use postit_data::posts::{
    LockedPost, NewPost, PostContent, PostFilter, PostMediaItem, PostRepo, PostStatus,
};
use postit_data::testkit::{Callers, callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire().await.unwrap()
}

async fn account(pool: &PgPool, owner: UserId, remote: &str) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, $3)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(remote)
    .execute(pool)
    .await
    .unwrap();
    AccountId::from_uuid(id)
}

fn new_post(text: &str) -> NewPost {
    NewPost {
        id: PostId::from_uuid(Uuid::now_v7()),
        kind: PostKind::Post,
        reply_to: None,
        campaign_id: None,
        text: text.to_string(),
        link: None,
        tags: vec![],
        disclose_ai_generated: false,
    }
}

fn content(text: &str) -> PostContent {
    PostContent {
        reply_to: None,
        campaign_id: None,
        text: text.to_string(),
        link: Some("https://example.com".to_string()),
        tags: vec!["rust".to_string()],
        disclose_ai_generated: true,
    }
}

fn new_media(id: MediaAssetId) -> NewMedia {
    NewMedia {
        id,
        storage_key: format!("media/{id}"),
        original_filename: None,
        mime: "image/png".to_string(),
        bytes: 1,
        status: MediaStatus::Ready,
        declared_bytes: None,
        upload_expires_at: None,
        derived_from: None,
        fit_spec: None,
    }
}

async fn version(pool: &PgPool, scope: &OwnerScope, id: PostId) -> i64 {
    let mut c = conn(pool).await;
    PostRepo::get(&mut c, scope, id).await.unwrap().version
}

async fn lock_in(
    pool: &PgPool,
    scope: &OwnerScope,
    id: PostId,
) -> (sqlx::Transaction<'static, sqlx::Postgres>, LockedPost) {
    let mut tx = pool.begin().await.unwrap();
    let lock = PostRepo::lock(&mut tx, scope, id, None).await.unwrap();
    (tx, lock)
}

async fn setup(pool: &PgPool, limited: &[AccountId]) -> (UserId, Callers) {
    let owner = seed_user(pool, "active").await;
    (owner, callers(pool, owner, limited).await)
}

async fn insert_job(pool: &PgPool, owner: UserId, post: PostId, dry_run: bool, status: &str) {
    sqlx::query(
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, dry_run, status) \
         VALUES ($1, $2, $3, 1, '{}', now(), $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(post.as_uuid())
    .bind(dry_run)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn post_isolation_matrix(pool: PgPool) {
    let owner_id = seed_user(&pool, "active").await;
    let a = account(&pool, owner_id, "a").await;
    let b = account(&pool, owner_id, "b").await;
    let who = callers(&pool, owner_id, &[a]).await;
    let mut c = conn(&pool).await;

    assert!(matches!(
        PostRepo::create(&mut c, &who.view, new_post("x")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let post = PostRepo::create(&mut c, &who.edit, new_post("hello"))
        .await
        .unwrap();
    assert_eq!(post.version, 1);
    assert_eq!(post.status, PostStatus::Draft);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[a, b])
        .await
        .unwrap();
    PostRepo::upsert_override(&mut tx, &lock, a, json!({"text": "A"}), json!({}))
        .await
        .unwrap();
    PostRepo::upsert_override(&mut tx, &lock, b, json!({"text": "B"}), json!({}))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            PostRepo::get(&mut c, scope, post.id).await.unwrap().text,
            "hello"
        );
        assert_eq!(
            PostRepo::list(&mut c, scope, page(), PostFilter::default())
                .await
                .unwrap()
                .total,
            1
        );
    }
    assert_eq!(
        PostRepo::targets(&mut c, &who.owner, post.id)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        PostRepo::targets(&mut c, &who.limited, post.id)
            .await
            .unwrap(),
        vec![a]
    );
    let seen = PostRepo::overrides(&mut c, &who.limited, post.id)
        .await
        .unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].account_id, a);

    assert!(matches!(
        PostRepo::get(&mut c, &who.stranger, post.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        PostRepo::targets(&mut c, &who.stranger, post.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        PostRepo::lock(&mut c, &who.stranger, post.id, None).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        PostRepo::lock(&mut c, &who.view, post.id, None).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert_eq!(
        PostRepo::list(&mut c, &who.stranger, page(), PostFilter::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[sqlx::test]
async fn list_filters_by_status_campaign_and_kind(pool: PgPool) {
    let (owner, who) = setup(&pool, &[]).await;
    let campaign = CampaignId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO campaigns (id, owner_id, name) VALUES ($1, $2, 'Spring')")
        .bind(campaign.as_uuid())
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut c = conn(&pool).await;
    let mut in_campaign = new_post("one");
    in_campaign.campaign_id = Some(campaign);
    PostRepo::create(&mut c, &who.owner, in_campaign)
        .await
        .unwrap();
    let mut comment = new_post("two");
    comment.kind = PostKind::Comment;
    PostRepo::create(&mut c, &who.owner, comment).await.unwrap();
    PostRepo::create(&mut c, &who.owner, new_post("three"))
        .await
        .unwrap();

    let mut total = async |filter| {
        PostRepo::list(&mut c, &who.owner, page(), filter)
            .await
            .unwrap()
            .total
    };
    assert_eq!(total(PostFilter::default()).await, 3);
    assert_eq!(
        total(PostFilter {
            campaign_id: Some(campaign),
            ..PostFilter::default()
        })
        .await,
        1
    );
    assert_eq!(
        total(PostFilter {
            kind: Some(PostKind::Comment),
            ..PostFilter::default()
        })
        .await,
        1
    );
    assert_eq!(
        total(PostFilter {
            status: Some(PostStatus::Published),
            ..PostFilter::default()
        })
        .await,
        0
    );

    let mut c = conn(&pool).await;
    let mut foreign = new_post("four");
    foreign.campaign_id = Some(CampaignId::from_uuid(Uuid::now_v7()));
    assert!(matches!(
        PostRepo::create(&mut c, &who.owner, foreign).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn every_write_bumps_version_once(pool: PgPool) {
    let (owner, who) = setup(&pool, &[]).await;
    let a = account(&pool, owner, "a").await;
    let asset = MediaAssetId::from_uuid(Uuid::now_v7());
    let mut c = conn(&pool).await;
    MediaRepo::insert(&mut c, &who.owner, new_media(asset))
        .await
        .unwrap();
    let post = PostRepo::create(&mut c, &who.owner, new_post("v1"))
        .await
        .unwrap();
    assert_eq!(post.version, 1);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::update_content(&mut tx, &lock, content("v2"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 2);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    let item = PostMediaItem {
        media_id: asset,
        position: 0,
        alt_text: Some("alt".to_string()),
    };
    PostRepo::set_media(&mut tx, &lock, &[item]).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 3);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[a]).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 4);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::upsert_override(&mut tx, &lock, a, json!({"text": "x"}), json!({}))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 5);

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::delete_override(&mut tx, &lock, a).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 6);

    let stored = PostRepo::get(&mut c, &who.owner, post.id).await.unwrap();
    assert_eq!(stored.text, "v2");
    assert_eq!(stored.tags, vec!["rust".to_string()]);
    assert!(stored.disclose_ai_generated);
    let updated_by: Option<Uuid> = sqlx::query_scalar("SELECT updated_by FROM posts WHERE id = $1")
        .bind(post.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(updated_by, Some(owner.as_uuid()));
}

#[sqlx::test]
async fn failed_write_leaves_version_alone(pool: PgPool) {
    let (_, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    let missing = AccountId::from_uuid(Uuid::now_v7());
    assert!(matches!(
        PostRepo::set_targets(&mut tx, &lock, &[missing]).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        PostRepo::delete_override(&mut tx, &lock, missing).await,
        Err(DataError::NotFound)
    ));
    tx.commit().await.unwrap();
    assert_eq!(version(&pool, &who.owner, post.id).await, 1);
}

#[sqlx::test]
async fn stale_version_conflicts(pool: PgPool) {
    let (_, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::update_content(&mut tx, &lock, content("y"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        PostRepo::lock(&mut c, &who.owner, post.id, Some(1)).await,
        Err(DataError::VersionConflict)
    ));
    PostRepo::lock(&mut c, &who.owner, post.id, Some(2))
        .await
        .unwrap();
}

#[sqlx::test]
async fn running_dry_run_locks_post(pool: PgPool) {
    let (owner, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    insert_job(&pool, owner, post.id, true, "publishing").await;

    assert!(matches!(
        PostRepo::lock(&mut c, &who.owner, post.id, Some(1)).await,
        Err(DataError::PostLocked)
    ));
    // The version check comes before the lock check.
    assert!(matches!(
        PostRepo::lock(&mut c, &who.owner, post.id, Some(9)).await,
        Err(DataError::VersionConflict)
    ));
    sqlx::query("UPDATE publish_jobs SET status = 'published'")
        .execute(&pool)
        .await
        .unwrap();
    PostRepo::lock(&mut c, &who.owner, post.id, Some(1))
        .await
        .unwrap();
}

#[sqlx::test]
async fn edit_pauses_recurrence(pool: PgPool) {
    let (owner, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO recurrences (id, owner_id, post_id, rule, timezone) \
         VALUES ($1, $2, $3, 'FREQ=DAILY', 'UTC')",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .bind(post.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::update_content(&mut tx, &lock, content("y"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (paused, reason): (bool, Option<String>) =
        sqlx::query_as("SELECT paused, paused_reason FROM recurrences WHERE post_id = $1")
            .bind(post.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(paused);
    assert_eq!(reason.as_deref(), Some("post_edited"));
}

#[sqlx::test]
async fn limited_set_targets_keeps_hidden_targets(pool: PgPool) {
    let owner_id = seed_user(&pool, "active").await;
    let a = account(&pool, owner_id, "a").await;
    let b = account(&pool, owner_id, "b").await;
    let who = callers(&pool, owner_id, &[a]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[a, b])
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.limited, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[]).await.unwrap();
    assert!(matches!(
        PostRepo::set_targets(&mut tx, &lock, &[b]).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        PostRepo::upsert_override(&mut tx, &lock, b, json!({}), json!({})).await,
        Err(DataError::NotFound)
    ));
    tx.commit().await.unwrap();
    assert_eq!(
        PostRepo::targets(&mut c, &who.owner, post.id)
            .await
            .unwrap(),
        vec![b]
    );
}

#[sqlx::test]
async fn outside_limit_detects_hidden_override(pool: PgPool) {
    let owner_id = seed_user(&pool, "active").await;
    let a = account(&pool, owner_id, "a").await;
    let b = account(&pool, owner_id, "b").await;
    let who = callers(&pool, owner_id, &[a]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    assert!(
        !PostRepo::outside_limit(&mut c, &who.limited, post.id)
            .await
            .unwrap()
    );

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::upsert_override(&mut tx, &lock, b, json!({"text": "B"}), json!({}))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        PostRepo::outside_limit(&mut c, &who.limited, post.id)
            .await
            .unwrap()
    );
    assert!(
        !PostRepo::outside_limit(&mut c, &who.owner, post.id)
            .await
            .unwrap()
    );
    assert!(
        !PostRepo::outside_limit(&mut c, &who.publish, post.id)
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn outside_limit_counts_hidden_targets_and_open_deliveries(pool: PgPool) {
    let owner_id = seed_user(&pool, "active").await;
    let a = account(&pool, owner_id, "a").await;
    let b = account(&pool, owner_id, "b").await;
    let who = callers(&pool, owner_id, &[a]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[b]).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        PostRepo::outside_limit(&mut c, &who.limited, post.id)
            .await
            .unwrap()
    );

    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    PostRepo::set_targets(&mut tx, &lock, &[]).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        !PostRepo::outside_limit(&mut c, &who.limited, post.id)
            .await
            .unwrap()
    );

    let job = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, status) \
         VALUES ($1, $2, $3, 1, '{}', now(), 'published')",
    )
    .bind(job)
    .bind(owner_id.as_uuid())
    .bind(post.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    for (state, expected) in [("pending", true), ("published", false)] {
        sqlx::query("DELETE FROM deliveries")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO deliveries (id, owner_id, job_id, post_id, account_id, operation, state, idempotency_key) \
             VALUES ($1, $2, $3, $4, $5, 'publish', $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(owner_id.as_uuid())
        .bind(job)
        .bind(post.id.as_uuid())
        .bind(b.as_uuid())
        .bind(state)
        .bind(Uuid::now_v7().as_bytes().to_vec())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            PostRepo::outside_limit(&mut c, &who.limited, post.id)
                .await
                .unwrap(),
            expected,
            "{state}"
        );
    }
}

#[sqlx::test]
async fn delete_with_live_history_needs_publish(pool: PgPool) {
    let (owner, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let draft = PostRepo::create(&mut c, &who.owner, new_post("draft"))
        .await
        .unwrap();
    let sent = PostRepo::create(&mut c, &who.owner, new_post("sent"))
        .await
        .unwrap();
    let rehearsed = PostRepo::create(&mut c, &who.owner, new_post("rehearsed"))
        .await
        .unwrap();
    insert_job(&pool, owner, sent.id, false, "published").await;
    insert_job(&pool, owner, rehearsed.id, true, "published").await;

    for post in [draft.id, rehearsed.id] {
        let (mut tx, lock) = lock_in(&pool, &who.edit, post).await;
        PostRepo::delete(&mut tx, lock).await.unwrap();
        tx.commit().await.unwrap();
    }
    let (mut tx, lock) = lock_in(&pool, &who.edit, sent.id).await;
    assert!(matches!(
        PostRepo::delete(&mut tx, lock).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    tx.rollback().await.unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.publish, sent.id).await;
    PostRepo::delete(&mut tx, lock).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        PostRepo::list(&mut c, &who.owner, page(), PostFilter::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[sqlx::test]
async fn comment_post_round_trips_reply_to(pool: PgPool) {
    let (_, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let reply = json!({"remote_post_id": "109", "remote_comment_id": "7"});
    let mut new = new_post("thanks!");
    new.kind = PostKind::Comment;
    new.reply_to = Some(reply.clone());
    let post = PostRepo::create(&mut c, &who.owner, new).await.unwrap();
    let stored = PostRepo::get(&mut c, &who.owner, post.id).await.unwrap();
    assert_eq!(stored.kind, PostKind::Comment);
    assert_eq!(stored.reply_to, Some(reply));
}

#[sqlx::test]
async fn set_media_rejects_another_owners_asset(pool: PgPool) {
    let (_, who) = setup(&pool, &[]).await;
    let stranger_asset = MediaAssetId::from_uuid(Uuid::now_v7());
    let mut c = conn(&pool).await;
    MediaRepo::insert(&mut c, &who.stranger, new_media(stranger_asset))
        .await
        .unwrap();
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();
    let (mut tx, lock) = lock_in(&pool, &who.owner, post.id).await;
    let item = PostMediaItem {
        media_id: stranger_asset,
        position: 0,
        alt_text: None,
    };
    assert!(matches!(
        PostRepo::set_media(&mut tx, &lock, &[item]).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn concurrent_lock_waits(pool: PgPool) {
    let (_, who) = setup(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let post = PostRepo::create(&mut c, &who.owner, new_post("x"))
        .await
        .unwrap();

    let mut first = pool.begin().await.unwrap();
    let lock = PostRepo::lock(&mut first, &who.owner, post.id, Some(1))
        .await
        .unwrap();

    let waiter = {
        let pool = pool.clone();
        let scope = who.owner.clone();
        let id = post.id;
        tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            PostRepo::lock(&mut tx, &scope, id, Some(1))
                .await
                .map(|_| ())
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !waiter.is_finished(),
        "second lock must wait for the first transaction"
    );

    PostRepo::update_content(&mut first, &lock, content("y"))
        .await
        .unwrap();
    first.commit().await.unwrap();
    let outcome = waiter.await.unwrap();
    assert!(matches!(outcome, Err(DataError::VersionConflict)));
}
