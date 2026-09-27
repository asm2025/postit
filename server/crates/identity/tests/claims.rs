#![cfg(feature = "testkit")]

use std::time::Duration;

use jsonwebtoken::Algorithm;
use postit_config::{BootstrapSettings, HttpSettings, UserinfoMode};
use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus, UsersRepo};
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::testkit::{TestIssuer, claims_transformer as transformer};
use postit_identity::verifier::Verifier;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

fn http_client() -> reqwest::Client {
    postit_http::build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"))
}

fn verifier(issuer: &TestIssuer) -> Verifier {
    Verifier::new(
        issuer
            .issuer_url()
            .to_string()
            .trim_end_matches('/')
            .to_string(),
        vec!["postit".to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    )
}

fn token(
    issuer: &TestIssuer,
    sub: &str,
    email: Option<&str>,
    email_verified: Option<bool>,
    name: Option<&str>,
) -> String {
    issuer.mint(
        &Claims {
            sub: sub.to_string(),
            iss: issuer
                .issuer_url()
                .to_string()
                .trim_end_matches('/')
                .to_string(),
            aud: "postit".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600,
            email: email.map(str::to_string),
            email_verified,
            name: name.map(str::to_string),
        },
        Algorithm::RS256,
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn first_sign_in_provisions_a_pending_member(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(
        &issuer,
        "sub-1",
        Some("ada@example.test"),
        Some(true),
        Some("Ada"),
    );
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(
        pool,
        &issuer,
        UserinfoMode::Never,
        BootstrapSettings {
            admin_email: None,
            admin_subject: None,
        },
    );

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
    assert_eq!(user.display_name, "Ada");
    assert_eq!(user.email.as_deref(), Some("ada@example.test"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bootstrap_promotes_a_newly_created_matching_user(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(
        &issuer,
        "admin-sub",
        Some("admin@example.test"),
        Some(true),
        Some("Admin"),
    );
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let bootstrap = BootstrapSettings {
        admin_email: Some("admin@example.test".to_string()),
        admin_subject: None,
    };
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, bootstrap);

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Admin);
    assert_eq!(user.status, UserStatus::Active);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bootstrap_ignores_admin_email_without_email_verified(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(
        &issuer,
        "admin-sub",
        Some("admin@example.test"),
        Some(false),
        Some("Admin"),
    );
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let bootstrap = BootstrapSettings {
        admin_email: Some("admin@example.test".to_string()),
        admin_subject: None,
    };
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, bootstrap);

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bootstrap_never_promotes_an_existing_user_who_starts_matching_later(pool: PgPool) {
    // Review Focus: only a newly created user is considered for bootstrap.
    let issuer = TestIssuer::start().await;
    let bearer = token(
        &issuer,
        "later-admin-sub",
        Some("later@example.test"),
        Some(true),
        Some("Later"),
    );

    // First sign-in: no bootstrap rule matches yet, so the user is provisioned as a
    // pending member.
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let no_bootstrap = BootstrapSettings {
        admin_email: None,
        admin_subject: None,
    };
    let transformer1 = transformer(pool.clone(), &issuer, UserinfoMode::Never, no_bootstrap);
    let first = transformer1
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("first transform: {e}"));
    assert_eq!(first.status, UserStatus::Pending);

    // Second sign-in with a config that now matches this already-existing user must not
    // promote them.
    let matching_bootstrap = BootstrapSettings {
        admin_email: Some("later@example.test".to_string()),
        admin_subject: None,
    };
    let transformer2 = transformer(pool, &issuer, UserinfoMode::Never, matching_bootstrap);
    let second = transformer2
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("second transform: {e}"));

    assert_eq!(second.id, first.id);
    assert_eq!(second.role, UserRole::Member);
    assert_eq!(second.status, UserStatus::Pending);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn userinfo_fills_a_claim_missing_from_the_access_token(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "sub-1", None, None, None);
    issuer
        .mount_userinfo(&bearer, serde_json::json!({"sub": "sub-1", "email": "from-userinfo@example.test", "email_verified": true, "name": "From Userinfo"}))
        .await;
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(
        pool,
        &issuer,
        UserinfoMode::Fallback,
        BootstrapSettings {
            admin_email: None,
            admin_subject: None,
        },
    );

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.email.as_deref(), Some("from-userinfo@example.test"));
    assert_eq!(user.display_name, "From Userinfo");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_userinfo_response_with_a_mismatched_sub_is_ignored(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "sub-1", None, None, None);
    issuer
        .mount_userinfo(&bearer, serde_json::json!({"sub": "someone-else", "email": "attacker@example.test", "email_verified": true}))
        .await;
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(
        pool,
        &issuer,
        UserinfoMode::Fallback,
        BootstrapSettings {
            admin_email: None,
            admin_subject: None,
        },
    );

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert!(user.email.is_none());
    assert_eq!(user.display_name, "sub-1");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn display_name_falls_back_through_preferred_username_then_email_then_subject(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = issuer.mint(
        &serde_json::json!({
            "sub": "sub-1",
            "iss": issuer.issuer_url().to_string().trim_end_matches('/'),
            "aud": "postit",
            "exp": chrono::Utc::now().timestamp() + 3600,
            "preferred_username": "adaverse",
            "email": "ada@example.test",
        }),
        Algorithm::RS256,
    );
    let verified = verifier(&issuer)
        .verify(&bearer, &issuer_jwks(&issuer).await)
        .unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(
        pool,
        &issuer,
        UserinfoMode::Never,
        BootstrapSettings {
            admin_email: None,
            admin_subject: None,
        },
    );

    let user = transformer
        .transform(&verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    // No `name` claim, so it falls to `preferred_username`.
    assert_eq!(user.display_name, "adaverse");
}

async fn issuer_jwks(issuer: &TestIssuer) -> jsonwebtoken::jwk::JwkSet {
    let client = http_client();
    let source = HttpJwksSource::new(client, issuer.issuer_url());
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("jwks: {e}"))
}

async fn send_email_rows(pool: &PgPool) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT payload FROM job_outbox WHERE job_type = 'send_email' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("outbox: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_pending_sign_up_enqueues_one_approval_email_per_active_admin(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bootstrap = BootstrapSettings {
        admin_email: None,
        admin_subject: Some("admin-1".into()),
    };
    let t = transformer(pool.clone(), &issuer, UserinfoMode::Never, bootstrap);

    // Bootstrap admin: nobody to notify, and it is not pending.
    let (admin_token, admin_claims) =
        issuer.token_and_claims("admin-1", Some("a@x.test"), Some(true), Some("Admin"));
    t.transform(&admin_claims, &admin_token)
        .await
        .unwrap_or_else(|e| unreachable!("admin: {e}"));
    assert!(send_email_rows(&pool).await.is_empty());

    // A second active admin.
    let second = UserId::from(uuid::Uuid::now_v7());
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::provision(&mut conn, second, "https://other.test", "admin-2", "Second")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, second)
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"));
    drop(conn);

    let (member_token, claims) =
        issuer.token_and_claims("member-1", Some("m@x.test"), Some(true), Some("Member"));
    let member = t
        .transform(&claims, &member_token)
        .await
        .unwrap_or_else(|e| unreachable!("member: {e}"));
    assert_eq!(member.status, UserStatus::Pending);

    let rows = send_email_rows(&pool).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["kind"] == "user_pending_approval"));

    // Signing in again does not notify again.
    t.transform(&claims, &member_token)
        .await
        .unwrap_or_else(|e| unreachable!("again: {e}"));
    assert_eq!(send_email_rows(&pool).await.len(), 2);
}
