#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{SECRET, credentials_grant, platforms, vault};
use postit_core::{AgentError, CredentialStore, Credentials, OAuthTokens};
use postit_data::OwnerScope;
use postit_data::accounts::{GrantRepo, GrantStatus};
use postit_data::testkit::seed_user;
use postit_oauth::PgCredentialStore;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn credentials_round_trip_plain_and_secret(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(pool.clone(), vault(), owner, grant);

    let Credentials::Fields(fields) = store.credentials().await.unwrap() else {
        panic!("expected fields");
    };

    assert_eq!(
        fields.get_plain("server_url").unwrap(),
        &serde_json::json!("https://example.test")
    );
    assert_eq!(fields.get_secret("token").unwrap().expose_secret(), SECRET);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn needs_reconnect_grant_is_authentication_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let mut conn = pool.acquire().await.unwrap();
    GrantRepo::set_status(
        &mut conn,
        &OwnerScope::own(owner),
        grant,
        GrantStatus::NeedsReconnect,
    )
    .await
    .unwrap();
    let store = PgCredentialStore::new(pool.clone(), vault(), owner, grant);

    let result = store.credentials().await;

    assert!(matches!(result, Err(AgentError::Authentication { .. })));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn another_owners_grant_is_authentication_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(pool.clone(), vault(), other, grant);

    let result = store.credentials().await;

    assert!(matches!(result, Err(AgentError::Authentication { .. })));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn store_refreshed_unsupported(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(pool.clone(), vault(), owner, grant);
    let tokens = OAuthTokens {
        access_token: SecretString::from("a".to_owned()),
        refresh_token: None,
        expires_at: None,
        scopes: vec![],
    };

    let result = store.store_refreshed(tokens).await;

    assert!(matches!(result, Err(AgentError::Unsupported { .. })));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn debug_of_store_and_credentials_never_contains_secret(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(pool.clone(), vault(), owner, grant);
    let credentials = store.credentials().await.unwrap();

    assert!(!format!("{store:?}").contains(SECRET));
    assert!(!format!("{credentials:?}").contains(SECRET));
}
