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
                    remote_id: "remote".to_owned(),
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

/// What [`seed_owned_rows`] created, for tests that need to point at one of the rows.
pub struct OwnedRows {
    pub post: PostId,
    pub account: AccountId,
    pub delivery: DeliveryId,
    pub direct_send: Uuid,
}

/// Gives `owner` one row in each owner-scoped table the publishing, email, and inbox features
/// write to. Every row's `created_by` and `updated_by` is `editor` where the table has them.
#[allow(clippy::too_many_lines)]
pub async fn seed_owned_rows(pool: &PgPool, owner: UserId, editor: UserId) -> OwnedRows {
    let scope = OwnerScope::own(owner);
    let post = post(pool, owner, "content").await;
    let account = account(pool, owner, &format!("acct-{}", owner.as_uuid())).await;
    let created = create_job(
        pool,
        &scope,
        post,
        &[account],
        at(Utc::now() - Duration::hours(1)),
    )
    .await;
    let delivery = created.deliveries[0].delivery;
    let (owner_id, delivery_id) = (owner.as_uuid(), delivery.as_uuid());

    sqlx::query("INSERT INTO delivery_attempts (id, owner_id, delivery_id) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(delivery_id)
        .execute(pool)
        .await
        .unwrap();
    let contact = Uuid::now_v7();
    sqlx::query("INSERT INTO contacts (id, owner_id, email, email_hash) VALUES ($1, $2, 'a@example.com', $3)")
        .bind(contact)
        .bind(owner_id)
        .bind(vec![1u8; 32])
        .execute(pool)
        .await
        .unwrap();
    let direct_send = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, contact_id, email_hash, state) \
         VALUES ($1, $2, $3, $4, $5, 'sent')",
    )
    .bind(direct_send)
    .bind(owner_id)
    .bind(delivery_id)
    .bind(contact)
    .bind(vec![1u8; 32])
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO email_events (id, owner_id, direct_send_id, kind) VALUES ($1, $2, $3, 'opened')")
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(direct_send)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO inbox_items (id, owner_id, account_id, remote_comment_id, text, received_at) \
         VALUES ($1, $2, $3, 'c1', 'hello', now())",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(account.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO post_metric_snapshots (id, owner_id, delivery_id, metric_offset) \
         VALUES ($1, $2, $3, '1h')",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(delivery_id)
    .execute(pool)
    .await
    .unwrap();
    let list = Uuid::now_v7();
    sqlx::query("INSERT INTO contact_lists (id, owner_id, name) VALUES ($1, $2, 'list')")
        .bind(list)
        .bind(owner_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO contact_list_members (list_id, contact_id, owner_id) VALUES ($1, $2, $3)",
    )
    .bind(list)
    .bind(contact)
    .bind(owner_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO suppressions (id, owner_id, email_hash, reason) VALUES ($1, $2, $3, 'manual')",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(vec![2u8; 32])
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_generations (id, owner_id, model, mode, prompt_hash, prompt) \
         VALUES ($1, $2, 'm', 'post', $3, 'secret prompt')",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(vec![3u8; 32])
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_imports (id, owner_id, status) VALUES ($1, $2, 'completed')")
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO notifications (id, user_id, owner_id, kind, read_at) VALUES ($1, $2, $2, 'user_pending', now())")
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .execute(pool)
        .await
        .unwrap();
    // The editor touched the owner's content.
    for table in [
        "posts",
        "publish_jobs",
        "deliveries",
        "contacts",
        "contact_lists",
        "suppressions",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET updated_by = $1 WHERE owner_id = $2"
        )))
        .bind(editor.as_uuid())
        .bind(owner_id)
        .execute(pool)
        .await
        .unwrap();
    }
    OwnedRows {
        post,
        account,
        delivery,
        direct_send,
    }
}

/// Waits until some statement in this database is blocked on a lock.
pub async fn wait_for_blocked_statement(pool: &PgPool) {
    loop {
        let blocked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \n             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if blocked > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
}

/// Runs `fut` for at most 5 seconds, so a lock the code under test fails to skip fails the
/// test instead of hanging CI.
pub async fn within<T>(fut: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), fut)
        .await
        .unwrap_or_else(|_| unreachable!("blocked: the lock under test is not skipped"))
}
