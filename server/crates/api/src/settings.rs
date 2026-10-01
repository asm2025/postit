use std::time::Duration;

use axum::http::HeaderValue;
use ipnet::IpNet;
use postit_config::{Environment, RateLimitSettings, Settings};
use url::Url;

use crate::client_ip::parse_trusted_proxies;

/// The slice of `Settings` the HTTP layer reads, validated once at startup.
#[derive(Debug, Clone)]
pub struct ApiSettings {
    pub environment: Environment,
    pub cors_origins: Vec<HeaderValue>,
    pub trusted_proxies: Vec<IpNet>,
    pub rate_limit: RateLimitSettings,
    pub request_timeout: Duration,
    pub body_limit: usize,
    pub issuer: Url,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub account_url: Option<Url>,
}

impl ApiSettings {
    /// # Errors
    ///
    /// Returns a message naming the bad key: a CORS origin that is `*` or not a valid header
    /// value, or an invalid `server.trusted_proxies` entry.
    pub fn from_settings(environment: Environment, s: &Settings) -> Result<Self, String> {
        let cors_origins =
            s.cors
                .allowed_origins
                .iter()
                .map(|o| {
                    // `AllowOrigin::list` panics on a wildcard; an explicit list is the contract.
                    if o.trim() == "*" {
                        return Err("cors.allowed_origins: `*` is not allowed; list each origin"
                            .to_string());
                    }
                    HeaderValue::from_str(o)
                        .map_err(|_| format!("cors.allowed_origins: invalid origin {o}"))
                })
                .collect::<Result<_, _>>()?;
        Ok(Self {
            environment,
            cors_origins,
            trusted_proxies: parse_trusted_proxies(&s.server.trusted_proxies)?,
            rate_limit: s.rate_limit.clone(),
            request_timeout: s.server.request_timeout,
            body_limit: s.server.body_limit,
            issuer: s.auth.oidc.issuer.clone(),
            client_id: s.auth.oidc.client_id.clone(),
            scopes: s.auth.oidc.scopes.clone(),
            account_url: s.auth.oidc.account_url.clone(),
        })
    }
}
