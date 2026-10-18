//! The vault and platform startup step against a real database.

use postit_core::AgentRegistry;
use postit_data::platforms::PlatformRepo;
use postit_server::Role;
use postit_server::platforms::{PlatformConfigs, PlatformStartup, check_vault_and_platforms};
use postit_vault::Vault;
use sqlx::PgPool;
use url::Url;

fn vault(version: u32) -> Vault {
    Vault::from_keys([(version, vec![7u8; 32])]).unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn public_url() -> Url {
    Url::parse("https://postit.test/").unwrap_or_else(|e| unreachable!("url: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn startup_with_no_agents_keeps_existing_rows(pool: PgPool) {
    sqlx::query("INSERT INTO platforms (id, display_name) VALUES ('tiktok', 'TikTok')")
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    let vault = vault(1);
    let registry = AgentRegistry::new();
    let configs = PlatformConfigs::default();
    let url = public_url();
    check_vault_and_platforms(
        &pool,
        &PlatformStartup {
            vault: &vault,
            role: Role::All,
            registry: &registry,
            configs: &configs,
            public_url: &url,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("startup: {e:#}"));
    let rows = PlatformRepo::list(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id.as_str(), "tiktok");
    assert_eq!(rows[0].display_name, "TikTok");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn startup_refuses_vault_behind_state(pool: PgPool) {
    sqlx::query("INSERT INTO vault_state (id, newest_key_version) VALUES (TRUE, 2)")
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    let vault = vault(1);
    let registry = AgentRegistry::new();
    let configs = PlatformConfigs::default();
    let url = public_url();
    for role in [Role::All, Role::Worker] {
        let err = check_vault_and_platforms(
            &pool,
            &PlatformStartup {
                vault: &vault,
                role,
                registry: &registry,
                configs: &configs,
                public_url: &url,
            },
        )
        .await
        .err()
        .unwrap_or_else(|| unreachable!("{role:?}: a vault behind vault_state must be refused"));
        let text = format!("{err:#}");
        assert!(text.contains("is behind the stored version 2"), "{text}");
    }
}
