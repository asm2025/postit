#![allow(clippy::unwrap_used)]

use chrono::{Duration, Utc};
use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, PlatformId, PostId, PublishJobId,
    UserId, UserPendingNotice,
};
use postit_data::delegations::{DelegationRepo, EndedReason};
use postit_data::testkit::{grant, seed_user};
use postit_data::{AccountLimit, Level};
use postit_notify::{Notifier, NotifyEvent, Recipients, notifications_for};
use sqlx::PgPool;
use uuid::Uuid;

async fn account(pool: &PgPool, owner: UserId) -> AccountId {
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
    .bind(id.to_string())
    .execute(pool)
    .await
    .unwrap();
    AccountId::from(id)
}

fn failed(account: AccountId) -> Notice {
    Notice::DeliveryFailed(DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: PublishJobId::from(Uuid::now_v7()),
        post_id: PostId::from(Uuid::now_v7()),
        account: AccountLabel {
            account_id: account,
            account_name: "Shop".to_owned(),
            platform: PlatformId::new("mastodon").unwrap(),
        },
        park_reason: None,
    })
}

fn workspace(owner: UserId, actor: Option<UserId>, notice: Notice) -> NotifyEvent {
    NotifyEvent {
        recipients: Recipients::Workspace { owner, actor },
        notice,
    }
}

async fn record(pool: &PgPool, event: NotifyEvent) -> usize {
    let notifier = Notifier::for_tests(pool.clone());
    let mut conn = pool.acquire().await.unwrap();
    notifier.record(&mut conn, event).await.unwrap().len()
}

async fn count(pool: &PgPool, user: UserId) -> usize {
    let mut conn = pool.acquire().await.unwrap();
    notifications_for(&mut conn, user).await.len()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn owner_is_notified(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let ids = record(&pool, workspace(owner, None, failed(acct))).await;
    assert_eq!(ids, 1);
    let mut conn = pool.acquire().await.unwrap();
    let rows = notifications_for(&mut conn, owner).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].owner, Some(owner));
    // A critical row waits for its alert email by default.
    assert_eq!(rows[0].emailed_at, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn live_delegate_actor_is_notified_too(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    grant(&pool, owner, delegate, Level::Publish, AccountLimit::All).await;
    let n = record(&pool, workspace(owner, Some(delegate), failed(acct))).await;
    assert_eq!(n, 2);
    assert_eq!(count(&pool, owner).await, 1);
    assert_eq!(count(&pool, delegate).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoked_delegate_gets_nothing_new(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let id = grant(&pool, owner, delegate, Level::Publish, AccountLimit::All).await;
    let mut conn = pool.acquire().await.unwrap();
    DelegationRepo::end(&mut conn, id, EndedReason::Revoked, Some(owner), Utc::now())
        .await
        .unwrap();
    drop(conn);
    let n = record(&pool, workspace(owner, Some(delegate), failed(acct))).await;
    assert_eq!(n, 1);
    assert_eq!(count(&pool, owner).await, 1);
    assert_eq!(count(&pool, delegate).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_delegate_gets_nothing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let id = grant(&pool, owner, delegate, Level::Publish, AccountLimit::All).await;
    sqlx::query("UPDATE delegations SET expires_at = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(Utc::now() - Duration::hours(1))
        .execute(&pool)
        .await
        .unwrap();
    let n = record(&pool, workspace(owner, Some(delegate), failed(acct))).await;
    assert_eq!(n, 1);
    assert_eq!(count(&pool, delegate).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_outside_limit_gets_nothing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let covered = account(&pool, owner).await;
    let other = account(&pool, owner).await;
    grant(
        &pool,
        owner,
        delegate,
        Level::Publish,
        AccountLimit::only([covered]),
    )
    .await;
    let n = record(&pool, workspace(owner, Some(delegate), failed(other))).await;
    assert_eq!(n, 1);
    assert_eq!(count(&pool, delegate).await, 0);
    let n = record(&pool, workspace(owner, Some(delegate), failed(covered))).await;
    assert_eq!(n, 2);
    assert_eq!(count(&pool, delegate).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn actor_equal_to_owner_gets_one_row(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let n = record(&pool, workspace(owner, Some(owner), failed(acct))).await;
    assert_eq!(n, 1);
    assert_eq!(count(&pool, owner).await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admins_get_deployment_notifications_with_null_owner(pool: PgPool) {
    let admin = seed_user(&pool, "active").await;
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(admin.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let pending = seed_user(&pool, "pending").await;
    let member = seed_user(&pool, "active").await;
    let notice = Notice::UserPending(UserPendingNotice { user_id: pending });
    let n = record(
        &pool,
        NotifyEvent {
            recipients: Recipients::Admins,
            notice,
        },
    )
    .await;
    assert_eq!(n, 1);
    let mut conn = pool.acquire().await.unwrap();
    let rows = notifications_for(&mut conn, admin).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].owner, None);
    assert_eq!(count(&pool, member).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn user_recipient_is_notified_with_optional_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let n = record(
        &pool,
        NotifyEvent {
            recipients: Recipients::User {
                user: delegate,
                owner: Some(owner),
            },
            notice: failed(acct),
        },
    )
    .await;
    assert_eq!(n, 1);
    let mut conn = pool.acquire().await.unwrap();
    let rows = notifications_for(&mut conn, delegate).await;
    assert_eq!(rows[0].owner, Some(owner));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn record_is_rolled_back_with_the_callers_transaction(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let acct = account(&pool, owner).await;
    let notifier = Notifier::for_tests(pool.clone());
    let mut tx = pool.begin().await.unwrap();
    let ids = notifier
        .record(&mut tx, workspace(owner, None, failed(acct)))
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, owner).await, 0);
}
