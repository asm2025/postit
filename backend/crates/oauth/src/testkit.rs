//! Test doubles for this crate's tests and the API's route tests: the stubs for services that
//! are not built yet and the helpers that assemble an [`crate::AccountService`]. The agent the
//! tests run against is `postit_agent_mock::MockAgent`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{Agent, ConnectMethod, MediaAsset};
use url::Url;

/// Media is not built yet (B4): every call says so.
pub struct NoMedia;

#[async_trait]
impl postit_core::MediaStore for NoMedia {
    async fn open(
        &self,
        _asset: &MediaAsset,
    ) -> Result<postit_core::ByteStream, postit_core::MediaError> {
        Err(postit_core::MediaError::NotFound)
    }
}

impl postit_core::MediaHost for NoMedia {
    fn public_url(
        &self,
        _asset: &MediaAsset,
        _ttl: Duration,
    ) -> Result<Url, postit_core::MediaError> {
        Err(postit_core::MediaError::NotReady)
    }
}

/// Link signing is not built yet (C2): every URL is a placeholder.
pub struct NoLinks;

impl postit_core::LinkSigner for NoLinks {
    fn unsubscribe_url(&self, _recipient: &postit_core::DirectRecipient) -> Url {
        placeholder()
    }

    fn click_url(&self, _send_id: postit_core::DirectSendId, _destination: &Url) -> Url {
        placeholder()
    }

    fn open_pixel_url(&self, _send_id: postit_core::DirectSendId) -> Url {
        placeholder()
    }
}

fn placeholder() -> Url {
    Url::parse("https://postit.test/unavailable").unwrap_or_else(|_| unreachable!())
}

/// The vault every test service uses.
///
/// # Panics
///
/// Never; the key is a constant of valid length.
#[must_use]
pub fn test_vault() -> postit_vault::Vault {
    postit_vault::Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

/// Default import limits for tests.
#[must_use]
pub fn test_accounts_settings() -> postit_config::AccountsSettings {
    postit_config::AccountsSettings {
        csv_max_bytes: 5_242_880,
        csv_max_rows: 1_000,
        csv_concurrency: 4,
        csv_timeout: Duration::from_secs(300),
    }
}

/// An [`crate::AccountService`] over `pool` and the given agents, with the test vault, the
/// system clock, and the not-yet-built media and link stubs.
///
/// # Panics
///
/// If an agent's platform is registered twice, or the HTTP clients cannot be built.
#[must_use]
pub fn account_service(
    pool: sqlx::PgPool,
    agents: Vec<Arc<dyn Agent>>,
    settings: postit_config::AccountsSettings,
) -> crate::AccountService {
    let mut registry = postit_core::AgentRegistry::new();
    for agent in agents {
        registry
            .register(agent)
            .unwrap_or_else(|e| unreachable!("register: {e}"));
    }
    let http_settings = postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-test/0".to_owned(),
        extra_ca_files: Vec::new(),
    };
    let standard =
        postit_http::build_client(&http_settings).unwrap_or_else(|e| unreachable!("client: {e}"));
    let public =
        postit_http::build_public_client(&http_settings, postit_http::AddressPolicy::strict())
            .unwrap_or_else(|e| unreachable!("client: {e}"));
    // OAuth platforms need a `[platforms.<id>]` table to be available; give them an empty one.
    let mut configs = crate::PlatformConfigs::default();
    for agent in registry.iter() {
        if matches!(agent.descriptor().connect, ConnectMethod::OAuth(_)) {
            configs.insert(agent.descriptor().platform.clone(), serde_json::json!({}));
        }
    }
    crate::AccountService::new(crate::AccountServiceDeps {
        pool,
        vault: test_vault(),
        catalog: Arc::new(crate::PlatformCatalog::new(registry, configs)),
        http: postit_http::agent_handles(standard, public),
        media_store: Arc::new(NoMedia),
        media_host: Arc::new(NoMedia),
        link_signer: Arc::new(NoLinks),
        clock: Arc::new(postit_core::SystemClock),
        ids: Arc::new(postit_core::SystemIdGenerator),
        settings,
    })
}
