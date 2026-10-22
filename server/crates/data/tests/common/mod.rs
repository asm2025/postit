//! Seeds shared by the publishing tests: an owner's accounts and posts, snapshots, and jobs.
#![allow(dead_code, clippy::unwrap_used)]

use chrono::{DateTime, Duration, Utc};
use postit_core::{
    AccountId, AccountRef, DeliveryId, PlatformId, PostId, PostKind, PostVersion, PublishJobId,
    PublishSnapshot, UserId, Variant,
};
use postit_data::OwnerScope;
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::{CreatedJob, JobRepo, NewDelivery, NewJob, Operation};
use sqlx::PgPool;
use uuid::Uuid;

/// Inserts a mastodon account for `owner` and returns its ID.
pub async fn account(pool: &PgPool, owner: UserId, remote: &str) -> AccountId {
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

/// Creates a draft post for `owner`.
pub async fn post(pool: &PgPool, owner: UserId, text: &str) -> PostId {
    let id = PostId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    PostRepo::create(
        &mut conn,
        &OwnerScope::own(owner),
        NewPost {
            id,
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: None,
            text: text.to_string(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    id
}

/// A snapshot of `post` at `version` with one variant per account.
pub fn snapshot(post: PostId, version: i64, accounts: &[AccountId]) -> PublishSnapshot {
    PublishSnapshot {
        post_id: post,
        post_version: PostVersion(version),
        variants: accounts
            .iter()
            .map(|account| Variant {
                target: AccountRef {
                    platform: PlatformId::new("mastodon").unwrap(),
                    account: *account,
                },
                kind: PostKind::Post,
                text: "hello".to_string(),
                media: vec![],
                link: None,
                tags: vec![],
                disclose_ai_generated: false,
                reply_to: None,
                email: None,
                options: serde_json::Map::new(),
            })
            .collect(),
    }
}

/// A live job for `accounts` that runs at `scheduled_at`.
pub fn new_job(
    post: PostId,
    version: i64,
    accounts: &[AccountId],
    scheduled_at: DateTime<Utc>,
) -> NewJob {
    NewJob {
        id: PublishJobId::from_uuid(Uuid::now_v7()),
        snapshot: snapshot(post, version, accounts),
        targets: accounts
            .iter()
            .map(|account| NewDelivery {
                id: DeliveryId::from_uuid(Uuid::now_v7()),
                account: *account,
                operation: Operation::Publish,
            })
            .collect(),
        scheduled_at,
        native_schedule: false,
        dry_run: false,
        recurrence_id: None,
    }
}

/// A time well in the future, cut to the microseconds Postgres stores.
pub fn later() -> DateTime<Utc> {
    at(Utc::now() + Duration::hours(1))
}

/// `time` cut to the microseconds Postgres stores, so it compares equal after a round trip.
pub fn at(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(time.timestamp_micros()).unwrap()
}

/// Locks `post`, creates a live job for `accounts` at `scheduled_at`, and commits.
pub async fn create_job(
    pool: &PgPool,
    scope: &OwnerScope,
    post: PostId,
    accounts: &[AccountId],
    scheduled_at: DateTime<Utc>,
) -> CreatedJob {
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, scope, post, None).await.unwrap();
    let version = locked.post().version;
    let created = JobRepo::create(
        &mut tx,
        scope,
        &locked,
        new_job(post, version, accounts, scheduled_at),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    created
}
