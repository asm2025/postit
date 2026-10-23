//! Platforms at startup (plan 03 A2): the deployment's `[platforms.<id>]` tables resolved
//! against the compiled-in agents, each platform's availability, and the vault and `platforms`
//! startup step.

use anyhow::Context;
use postit_core::{AgentRegistry, ConnectMethod, PlatformId};
use postit_data::platforms::PlatformRepo;
use postit_vault::Vault;
use sqlx::PgPool;
use url::Url;

use crate::Role;

pub use postit_oauth::catalog::{
    Availability, PlatformConfigs, availability, resolve_platform_configs,
};

/// The OAuth callback a platform's developer app must register:
/// `{public_url}/api/v1/platforms/{id}/oauth/callback`, keeping any base path.
#[must_use]
pub fn oauth_redirect_uri(public_url: &Url, id: &PlatformId) -> Url {
    let mut url = public_url.clone();
    let base = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base}/api/v1/platforms/{id}/oauth/callback"));
    url.set_query(None);
    url.set_fragment(None);
    url
}

/// What the vault and platform startup step needs.
pub struct PlatformStartup<'a> {
    /// The configured vault.
    pub vault: &'a Vault,
    /// Which parts of the process run.
    pub role: Role,
    /// The compiled-in agents.
    pub registry: &'a AgentRegistry,
    /// The resolved platform tables.
    pub configs: &'a PlatformConfigs,
    /// `server.public_url`, the base of OAuth redirect URIs.
    pub public_url: &'a Url,
}

/// Checks the vault against the database in every role. In API roles it then records the
/// compiled-in platforms and logs each unavailable platform and each OAuth redirect URI.
///
/// # Errors
///
/// A vault behind the database's newest key version, a sealed value under a key the vault
/// lacks, or a database failure.
pub async fn check_vault_and_platforms(
    pool: &PgPool,
    startup: &PlatformStartup<'_>,
) -> anyhow::Result<()> {
    postit_data::vault::vault_check_startup(
        pool,
        startup.vault,
        postit_data::vault::SEALED_COLUMNS,
    )
    .await
    .context("checking the vault keys against the database")?;
    if !startup.role.runs_api() {
        return Ok(());
    }
    let registered: Vec<(PlatformId, String)> = startup
        .registry
        .iter()
        .map(|agent| {
            let d = agent.descriptor();
            (d.platform.clone(), d.display_name.clone())
        })
        .collect();
    PlatformRepo::upsert_registered(pool, &registered)
        .await
        .context("recording the registered platforms")?;
    let rows = PlatformRepo::list(pool)
        .await
        .context("listing platforms")?;
    for row in &rows {
        let state = availability(row, startup.registry, startup.configs);
        if state != Availability::Available {
            tracing::warn!(platform = %row.id, availability = ?state, "platform is not available");
        }
    }
    for agent in startup.registry.iter() {
        let d = agent.descriptor();
        if matches!(d.connect, ConnectMethod::OAuth(_)) {
            tracing::info!(
                platform = %d.platform,
                redirect_uri = %oauth_redirect_uri(startup.public_url, &d.platform),
                "register this OAuth redirect URI with the platform's developer app"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::PlatformId;
    use url::Url;

    use super::*;

    fn id(s: &str) -> PlatformId {
        PlatformId::new(s).unwrap()
    }

    #[test]
    fn redirect_uri_keeps_base_path() {
        let base = Url::parse("https://h/x/").unwrap();
        assert_eq!(
            oauth_redirect_uri(&base, &id("tiktok")).as_str(),
            "https://h/x/api/v1/platforms/tiktok/oauth/callback"
        );
        let root = Url::parse("https://h").unwrap();
        assert_eq!(
            oauth_redirect_uri(&root, &id("tiktok")).as_str(),
            "https://h/api/v1/platforms/tiktok/oauth/callback"
        );
        let no_slash = Url::parse("https://h/x").unwrap();
        assert_eq!(
            oauth_redirect_uri(&no_slash, &id("tiktok")).as_str(),
            "https://h/x/api/v1/platforms/tiktok/oauth/callback"
        );
    }
}
