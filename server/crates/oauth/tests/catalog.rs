#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::{platform, platforms};
use postit_core::AgentRegistry;
use postit_data::platforms::PlatformRepo;
use postit_oauth::testkit::TestCredentialAgent;
use postit_oauth::{AccountError, PlatformCatalog, PlatformConfigs};
use sqlx::PgPool;

fn catalog(agents: Vec<Arc<TestCredentialAgent>>) -> PlatformCatalog {
    let mut registry = AgentRegistry::new();
    for agent in agents {
        registry.register(agent).unwrap();
    }
    PlatformCatalog::new(registry, PlatformConfigs::default())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_platform_not_found(pool: PgPool) {
    let catalog = catalog(vec![TestCredentialAgent::new("fake")]);
    platforms(&pool, &["fake", "gone"]).await;
    let mut conn = pool.acquire().await.unwrap();

    // A row exists but this build has no agent for it.
    let gone = catalog.resolve(&mut conn, &platform("gone")).await;
    // No `platforms` row at all.
    let nothing = catalog.resolve(&mut conn, &platform("never")).await;

    assert!(matches!(gone, Err(AccountError::PlatformNotFound)));
    assert!(matches!(nothing, Err(AccountError::PlatformNotFound)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_platform_refused(pool: PgPool) {
    let catalog = catalog(vec![TestCredentialAgent::new("fake")]);
    platforms(&pool, &["fake"]).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(catalog.resolve(&mut conn, &platform("fake")).await.is_ok());

    PlatformRepo::set_enabled(&pool, &platform("fake"), false)
        .await
        .unwrap();

    // The second call sees the flag: nothing is cached.
    let result = catalog.resolve(&mut conn, &platform("fake")).await;
    assert!(matches!(result, Err(AccountError::PlatformDisabled)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unconfigured_platform_unavailable(pool: PgPool) {
    // An OAuth agent needs a `[platforms.<id>]` table; this deployment has none.
    let catalog = catalog(vec![TestCredentialAgent::oauth("fake")]);
    platforms(&pool, &["fake"]).await;
    let mut conn = pool.acquire().await.unwrap();

    let result = catalog.resolve(&mut conn, &platform("fake")).await;

    assert!(matches!(result, Err(AccountError::PlatformUnavailable)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn available_platform_resolves(pool: PgPool) {
    let catalog = catalog(vec![TestCredentialAgent::new("fake")]);
    platforms(&pool, &["fake"]).await;
    let mut conn = pool.acquire().await.unwrap();

    let resolved = catalog.resolve(&mut conn, &platform("fake")).await.unwrap();

    assert_eq!(resolved.agent.descriptor().platform.as_str(), "fake");
    assert_eq!(resolved.config, serde_json::json!({}));
}
