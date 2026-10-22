#![allow(clippy::unwrap_used)]

use emixdb::dto::Pagination;
use postit_core::{AccountId, TargetGroupId, UserId};
use postit_data::target_groups::TargetGroupRepo;
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

fn id() -> TargetGroupId {
    TargetGroupId::from_uuid(Uuid::now_v7())
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

#[sqlx::test]
async fn target_group_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        TargetGroupRepo::create(&mut c, &who.view, id(), "g", &[a]).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let group = TargetGroupRepo::create(&mut c, &who.edit, id(), "Everywhere", &[a, b])
        .await
        .unwrap();
    assert_eq!(group.accounts.len(), 2);

    for scope in [&who.owner, &who.view, &who.edit, &who.publish] {
        assert_eq!(
            TargetGroupRepo::get(&mut c, scope, group.id)
                .await
                .unwrap()
                .accounts
                .len(),
            2
        );
    }
    let limited = TargetGroupRepo::get(&mut c, &who.limited, group.id)
        .await
        .unwrap();
    assert_eq!(limited.accounts, vec![a]);
    let listed = TargetGroupRepo::list(&mut c, &who.limited, page())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(listed.data[0].accounts, vec![a]);

    assert!(matches!(
        TargetGroupRepo::rename(&mut c, &who.view, group.id, "x").await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        TargetGroupRepo::get(&mut c, &who.stranger, group.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        TargetGroupRepo::rename(&mut c, &who.stranger, group.id, "x").await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        TargetGroupRepo::set_accounts(&mut c, &who.stranger, group.id, &[]).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        TargetGroupRepo::delete(&mut c, &who.stranger, group.id).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        TargetGroupRepo::list(&mut c, &who.stranger, page())
            .await
            .unwrap()
            .total,
        0
    );
}

#[sqlx::test]
async fn limited_delegate_target_group_keeps_hidden_members(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut c = pool.acquire().await.unwrap();
    let group = TargetGroupRepo::create(&mut c, &who.owner, id(), "g", &[a, b])
        .await
        .unwrap();

    let after = TargetGroupRepo::set_accounts(&mut c, &who.limited, group.id, &[])
        .await
        .unwrap();
    assert!(after.accounts.is_empty());
    let full = TargetGroupRepo::get(&mut c, &who.owner, group.id)
        .await
        .unwrap();
    assert_eq!(full.accounts, vec![b]);

    assert!(matches!(
        TargetGroupRepo::set_accounts(&mut c, &who.limited, group.id, &[b]).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        TargetGroupRepo::create(&mut c, &who.limited, id(), "h", &[b]).await,
        Err(DataError::NotFound)
    ));
    let still = TargetGroupRepo::get(&mut c, &who.owner, group.id)
        .await
        .unwrap();
    assert_eq!(still.accounts, vec![b]);
}

#[sqlx::test]
async fn set_accounts_replaces_members_and_ignores_repeats(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let group = TargetGroupRepo::create(&mut c, &who.owner, id(), "g", &[a])
        .await
        .unwrap();

    let after = TargetGroupRepo::set_accounts(&mut c, &who.owner, group.id, &[b, b])
        .await
        .unwrap();
    assert_eq!(after.accounts, vec![b]);
    let renamed = TargetGroupRepo::rename(&mut c, &who.edit, group.id, "renamed")
        .await
        .unwrap();
    assert_eq!(renamed.name, "renamed");
    assert_eq!(renamed.accounts, vec![b]);
}

#[sqlx::test]
async fn another_owners_account_is_rejected_and_nothing_is_written(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let theirs = account(&pool, other, "theirs").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        TargetGroupRepo::create(&mut c, &who.owner, id(), "g", &[theirs]).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        TargetGroupRepo::list(&mut c, &who.owner, page())
            .await
            .unwrap()
            .total,
        0
    );
}

#[sqlx::test]
async fn deleting_an_account_drops_it_from_groups(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let group = TargetGroupRepo::create(&mut c, &who.owner, id(), "g", &[a])
        .await
        .unwrap();

    sqlx::query("DELETE FROM connected_accounts WHERE id = $1")
        .bind(a.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let after = TargetGroupRepo::get(&mut c, &who.owner, group.id)
        .await
        .unwrap();
    assert!(after.accounts.is_empty());
    TargetGroupRepo::delete(&mut c, &who.edit, group.id)
        .await
        .unwrap();
}
