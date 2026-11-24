#![allow(clippy::unwrap_used)]

use chrono::{Duration, Utc};
use postit_core::{PendingConnectionId, PlatformId};
use postit_data::pending_connections::{NewPendingConnection, PendingConnectionRepo};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use postit_vault::Vault;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn platform() -> PlatformId {
    PlatformId::new("mastodon").unwrap_or_else(|e| unreachable!("platform: {e}"))
}

async fn platforms(pool: &PgPool) {
    sqlx::query("INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'mastodon')")
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("platform: {e}"));
}

fn new_pending(hash: [u8; 32], expires_in: Duration) -> NewPendingConnection {
    let id = PendingConnectionId::from(Uuid::now_v7());
    let credential = vault()
        .seal(
            &json!({"token": "secret"}),
            &PendingConnectionRepo::seal_context(id),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    NewPendingConnection {
        id,
        platform: platform(),
        connect_id_hash: hash,
        credential,
        connection_config: json!({"server_url": "https://example.test"}),
        discovered: json!([]),
        expires_at: Utc::now() + expires_in,
    }
}

#[sqlx::test]
async fn take_is_single_use(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    PendingConnectionRepo::insert(&mut c, &scope, new_pending([1; 32], Duration::minutes(10)))
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let first = PendingConnectionRepo::take(&mut c, &[1; 32]).await.unwrap();
    let second = PendingConnectionRepo::take(&mut c, &[1; 32]).await.unwrap();

    let first = first.unwrap_or_else(|| unreachable!("row"));
    assert_eq!(first.owner, owner);
    assert_eq!(
        first.connection_config["server_url"],
        "https://example.test"
    );
    assert!(second.is_none());
}

#[sqlx::test]
async fn take_returns_another_owners_row_and_deletes_it(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    PendingConnectionRepo::insert(
        &mut c,
        &OwnerScope::own(owner),
        new_pending([2; 32], Duration::minutes(10)),
    )
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));

    // `take` knows no caller: the service compares the owner.
    let taken = PendingConnectionRepo::take(&mut c, &[2; 32]).await.unwrap();

    assert_eq!(taken.map(|p| p.owner), Some(owner));
    assert!(
        PendingConnectionRepo::take(&mut c, &[2; 32])
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn take_returns_expired_row(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    PendingConnectionRepo::insert(
        &mut c,
        &OwnerScope::own(owner),
        new_pending([3; 32], Duration::minutes(-1)),
    )
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let taken = PendingConnectionRepo::take(&mut c, &[3; 32]).await.unwrap();

    let taken = taken.unwrap_or_else(|| unreachable!("row"));
    assert!(taken.expires_at < Utc::now());
}

#[sqlx::test]
async fn insert_requires_owner(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = PendingConnectionRepo::insert(
            &mut c,
            scope,
            new_pending([4; 32], Duration::minutes(10)),
        )
        .await;
        assert!(matches!(
            result,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
}

#[sqlx::test]
async fn credential_is_sealed_for_its_row(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let new = new_pending([5; 32], Duration::minutes(10));
    let id = new.id;
    PendingConnectionRepo::insert(&mut c, &OwnerScope::own(owner), new)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let taken = PendingConnectionRepo::take(&mut c, &[5; 32])
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("row"));

    let opened: Value = vault()
        .open(&taken.credential, &PendingConnectionRepo::seal_context(id))
        .unwrap_or_else(|e| unreachable!("open: {e}"));
    assert_eq!(opened["token"], "secret");
    let other = PendingConnectionRepo::seal_context(PendingConnectionId::from(Uuid::now_v7()));
    assert!(vault().open::<Value>(&taken.credential, &other).is_err());
}

#[sqlx::test]
async fn list_live_returns_only_owner_live_rows(pool: PgPool) {
    platforms(&pool).await;
    let a = seed_user(&pool, "active").await;
    let b = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let live_a = new_pending([10; 32], Duration::minutes(10));
    let live_a_id = live_a.id;
    PendingConnectionRepo::insert(&mut c, &OwnerScope::own(a), live_a)
        .await
        .unwrap();
    PendingConnectionRepo::insert(
        &mut c,
        &OwnerScope::own(a),
        new_pending([11; 32], Duration::minutes(-1)),
    )
    .await
    .unwrap();
    let live_b = new_pending([12; 32], Duration::minutes(10));
    let live_b_id = live_b.id;
    PendingConnectionRepo::insert(&mut c, &OwnerScope::own(b), live_b)
        .await
        .unwrap();

    let mine = PendingConnectionRepo::list_live(&mut c, &OwnerScope::own(a), Utc::now())
        .await
        .unwrap();
    let theirs = PendingConnectionRepo::list_live(&mut c, &OwnerScope::own(b), Utc::now())
        .await
        .unwrap();

    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, live_a_id.as_uuid());
    assert_eq!(mine[0].platform, platform());
    assert!(mine[0].expires_at > mine[0].created_at);
    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].id, live_b_id.as_uuid());
}

#[sqlx::test]
async fn delete_owned_refuses_foreign_and_expired(pool: PgPool) {
    platforms(&pool).await;
    let a = seed_user(&pool, "active").await;
    let b = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let live = new_pending([20; 32], Duration::minutes(10));
    let live_id = live.id;
    PendingConnectionRepo::insert(&mut c, &OwnerScope::own(a), live)
        .await
        .unwrap();
    let expired = new_pending([21; 32], Duration::minutes(-1));
    let expired_id = expired.id;
    PendingConnectionRepo::insert(&mut c, &OwnerScope::own(a), expired)
        .await
        .unwrap();

    let foreign =
        PendingConnectionRepo::delete_owned(&mut c, &OwnerScope::own(b), live_id, Utc::now())
            .await
            .unwrap();
    let late =
        PendingConnectionRepo::delete_owned(&mut c, &OwnerScope::own(a), expired_id, Utc::now())
            .await
            .unwrap();

    assert!(!foreign);
    assert!(!late);
    assert!(
        PendingConnectionRepo::find(&mut c, &[20; 32])
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        PendingConnectionRepo::find(&mut c, &[21; 32])
            .await
            .unwrap()
            .is_some()
    );
    let own = PendingConnectionRepo::delete_owned(&mut c, &OwnerScope::own(a), live_id, Utc::now())
        .await
        .unwrap();
    assert!(own);
    assert!(
        PendingConnectionRepo::find(&mut c, &[20; 32])
            .await
            .unwrap()
            .is_none()
    );
}
