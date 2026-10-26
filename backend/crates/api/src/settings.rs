//! The slice of settings the HTTP layer reads.

use std::time::Duration;

use axum::http::HeaderValue;
use ipnet::IpNet;
use postit_config::{Environment, RateLimitSettings, Settings};
use url::Url;

use crate::client_ip::parse_trusted_proxies;

/// The slice of `Settings` the HTTP layer reads, validated once at startup.
#[derive(Debug, Clone)]
pub struct ApiSettings {
    /// Deployment environment.
    pub environment: Environment,
    /// Origins allowed to call the API from a browser.
    pub cors_origins: Vec<HeaderValue>,
    /// Proxies whose forwarded headers are trusted.
    pub trusted_proxies: Vec<IpNet>,
    /// Rate limits per class.
    pub rate_limit: RateLimitSettings,
    /// Per-request timeout.
    pub request_timeout: Duration,
    /// Largest request body, in bytes.
    pub body_limit: usize,
    /// Issuer of the identity provider.
    pub issuer: Url,
    /// OAuth client ID of the web client.
    pub client_id: String,
    /// OAuth client ID of the Android app, when configured.
    pub android_client_id: Option<String>,
    /// OAuth client ID of the iOS app, when configured.
    pub ios_client_id: Option<String>,
    /// Scopes handed to clients.
    pub scopes: Vec<String>,
    /// Link to the provider's account page, when configured.
    pub account_url: Option<Url>,
    /// Largest CSV body the account import accepts, in bytes.
    pub csv_max_bytes: usize,
    /// Longest the account import request may run.
    pub csv_timeout: Duration,
    /// Largest body of a single-request media upload, in bytes.
    pub media_single_request_max: usize,
    /// Largest chunk of a resumable media upload, in bytes.
    pub media_chunk_max: usize,
    /// How long a media upload or download may sit idle before it is cut off.
    pub media_idle_timeout: Duration,
    /// Longest the media completion request may run.
    pub media_complete_timeout: Duration,
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
            android_client_id: s.auth.oidc.android_client_id.clone(),
            ios_client_id: s.auth.oidc.ios_client_id.clone(),
            scopes: s.auth.oidc.scopes.clone(),
            account_url: s.auth.oidc.account_url.clone(),
            csv_max_bytes: usize::try_from(s.accounts.csv_max_bytes).unwrap_or(usize::MAX),
            csv_timeout: s.accounts.csv_timeout,
            media_single_request_max: usize::try_from(s.media.single_request_max)
                .unwrap_or(usize::MAX),
            media_chunk_max: usize::try_from(s.media.chunk_max).unwrap_or(usize::MAX),
            media_idle_timeout: s.media.idle_timeout,
            media_complete_timeout: s.media.complete_timeout,
        })
    }
}
