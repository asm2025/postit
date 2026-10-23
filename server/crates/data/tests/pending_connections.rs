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
