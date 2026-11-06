//! Builds a provider from a stored row: the seam between `DrafterService` and the protocols, so
//! tests can swap in a scripted provider.

use std::sync::Arc;

use postit_http::PublicClient;
use url::Url;

use crate::anthropic::Anthropic;
use crate::config::{Protocol, ProviderConfig, ProviderSecret, normalize_base};
use crate::error::DrafterError;
use crate::openai::OpenAiCompatible;
use crate::presets::Preset;
use crate::provider::LlmProvider;

/// Turns a provider's config and opened secret into something that can be called.
pub trait ProviderFactory: Send + Sync {
    /// Builds the provider. The secret is moved in and lives only inside the provider.
    ///
    /// # Errors
    /// [`DrafterError::Validation`] when the config names no protocol or no base URL.
    fn build(
        &self,
        config: &ProviderConfig,
        secret: ProviderSecret,
        preset: Option<&'static Preset>,
    ) -> Result<Arc<dyn LlmProvider>, DrafterError>;
}

/// The real factory: both protocols over one public client.
#[derive(Debug, Clone)]
pub struct HttpProviderFactory {
    client: PublicClient,
}

impl HttpProviderFactory {
    /// A factory sending through `client`, whose address policy follows
    /// `ai.allow_private_endpoints`.
    #[must_use]
    pub fn new(client: PublicClient) -> Self {
        Self { client }
    }
}

/// The base URL: the config's, else the preset's.
fn base_url(
    config: &ProviderConfig,
    preset: Option<&'static Preset>,
    protocol: Protocol,
) -> Result<Url, DrafterError> {
    let url = match (&config.options.base_url, preset) {
        (Some(url), _) => url.clone(),
        (None, Some(p)) => Url::parse(p.base_url).map_err(|_| missing_base())?,
        (None, None) => return Err(missing_base()),
    };
    Ok(normalize_base(protocol, url))
}

fn missing_base() -> DrafterError {
    DrafterError::Validation(vec![
        postit_core::Violation::agent_rule(postit_core::Message::new_static(
            "ai.base_url_required",
            "The provider needs a base address.",
        ))
        .at("options.baseURL"),
    ])
}

impl ProviderFactory for HttpProviderFactory {
    fn build(
        &self,
        config: &ProviderConfig,
        secret: ProviderSecret,
        preset: Option<&'static Preset>,
    ) -> Result<Arc<dyn LlmProvider>, DrafterError> {
        let protocol = config
            .protocol()
            .or_else(|v| preset.map(|p| p.protocol).ok_or(v))
            .map_err(|v| DrafterError::Validation(vec![v]))?;
        let base = base_url(config, preset, protocol)?;
        Ok(match protocol {
            Protocol::OpenAiCompatible => Arc::new(OpenAiCompatible::new(
                self.client.clone(),
                base,
                secret,
                config,
                preset,
            )),
            Protocol::Anthropic => {
                Arc::new(Anthropic::new(self.client.clone(), base, secret, config))
            }
        })
    }
}
