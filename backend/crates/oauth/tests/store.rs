#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{SECRET, credentials_grant, platforms};
use postit_core::{AgentError, CredentialStore, Credentials, OAuthTokens};
use postit_data::OwnerScope;
use postit_data::accounts::{GrantRepo, GrantStatus};
use postit_data::testkit::seed_user;
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountService, PgCredentialStore};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

/// A service with no agents: a credentials grant is read without one.
fn service(pool: &PgPool) -> AccountService {
    account_service(pool.clone(), vec![], test_accounts_settings())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn credentials_round_trip_plain_and_secret(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(service(&pool), owner, grant);

    let Credentials::Fields(fields) = store.credentials().await.unwrap() else {
        panic!("expected fields");
    };

    assert_eq!(
        fields.get_plain("account_name").unwrap(),
        &serde_json::json!("test-account")
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
    let store = PgCredentialStore::new(service(&pool), owner, grant);

    let result = store.credentials().await;

    assert!(matches!(result, Err(AgentError::Authentication { .. })));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_without_credential_is_authentication_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    sqlx::query("UPDATE account_grants SET credential = NULL WHERE id = $1")
        .bind(grant.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let store = PgCredentialStore::new(service(&pool), owner, grant);

    let result = store.credentials().await;

    assert!(matches!(result, Err(AgentError::Authentication { .. })));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn another_owners_grant_is_authentication_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(service(&pool), other, grant);

    let result = store.credentials().await;

    assert!(matches!(result, Err(AgentError::Authentication { .. })));
}

/// A database failure while reading the grant is a transport error to retry, never a reason
/// to send the user to reconnect.
#[sqlx::test(migrations = "../data/migrations")]
async fn database_failure_reading_the_grant_is_transport_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(service(&pool), owner, grant);
    sqlx::query("ALTER TABLE account_grants RENAME TO broken_table")
        .execute(&pool)
        .await
        .unwrap();

    let result = store.credentials().await;

    sqlx::query("ALTER TABLE broken_table RENAME TO account_grants")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        matches!(result, Err(AgentError::Transport { .. })),
        "{result:?}"
    );
    assert!(store.credentials().await.is_ok());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn store_refreshed_unsupported(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let grant = credentials_grant(&pool, owner, "fake").await;
    let store = PgCredentialStore::new(service(&pool), owner, grant);
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
    let store = PgCredentialStore::new(service(&pool), owner, grant);
    let credentials = store.credentials().await.unwrap();

    assert!(!format!("{store:?}").contains(SECRET));
    assert!(!format!("{credentials:?}").contains(SECRET));
}

/// An active OAuth grant of `owner` whose access token expires at `expires_at`.
async fn oauth_grant(
    pool: &PgPool,
    owner: postit_core::UserId,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> postit_core::GrantId {
    let id = postit_core::GrantId::from(uuid::Uuid::now_v7());
    let credential = common::vault()
        .seal(
            &serde_json::json!({"access_token": SECRET, "refresh_token": "refresh-value"}),
            &GrantRepo::seal_context(id),
        )
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    GrantRepo::insert(
        &mut conn,
        &OwnerScope::own(owner),
        postit_data::accounts::NewGrant {
            id,
            platform: common::platform("fake"),
            kind: postit_data::accounts::GrantKind::Oauth,
            remote_user_id: None,
            credential,
            connection_config: serde_json::json!({}),
            expires_at,
            reconnect_by: None,
            scopes: vec!["read".to_owned()],
        },
    )
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_grant_serves_its_tokens(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let expires = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp() + 3600, 0);
    let grant = oauth_grant(&pool, owner, expires).await;
    // No agent is registered: an unexpired token is served without any platform call.
    let store = PgCredentialStore::new(service(&pool), owner, grant);

    let Credentials::OAuth(tokens) = store.credentials().await.unwrap() else {
        panic!("expected OAuth tokens");
    };
    assert_eq!(tokens.access_token.expose_secret(), SECRET);
    assert_eq!(
        tokens.refresh_token.unwrap().expose_secret(),
        "refresh-value"
    );
    assert_eq!(tokens.expires_at, expires);
    assert_eq!(tokens.scopes, ["read"]);
    assert!(!format!("{store:?}").contains(SECRET));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_grant_of_another_owner_is_authentication_error(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let grant = oauth_grant(&pool, owner, None).await;
    let store = PgCredentialStore::new(service(&pool), other, grant);
    let tokens = OAuthTokens {
        access_token: SecretString::from(SECRET.to_owned()),
        refresh_token: None,
        expires_at: None,
        scopes: vec![],
    };

    assert!(matches!(
        store.credentials().await,
        Err(AgentError::Authentication { .. })
    ));
    assert!(matches!(
        store.refresh(&tokens).await,
        Err(AgentError::Authentication { .. })
    ));
    assert!(matches!(
        store.store_refreshed(tokens).await,
        Err(AgentError::Authentication { .. })
    ));
}
