use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::jwk::JwkSet;
use postit_http::HttpError;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryDocument {
    pub issuer: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
}

/// Separates "fetch the discovery document and JWKS over HTTP" from the caching and
/// unknown-`kid` refetch logic in [`OidcDiscovery`], so tests can substitute a source that
/// doesn't hit the network. `postit-identity`'s own `testkit` feature doesn't need a second
/// implementation of this trait — its test issuer (Task 13) serves real HTTP responses
/// through wiremock, so [`HttpJwksSource`] is exercised as-is in tests too.
#[async_trait]
pub trait JwksSource: Send + Sync {
    async fn discovery(&self) -> Result<DiscoveryDocument, HttpError>;
    async fn jwks(&self) -> Result<JwkSet, HttpError>;
}

pub struct HttpJwksSource {
    client: reqwest::Client,
    issuer: Url,
}

impl HttpJwksSource {
    #[must_use]
    pub fn new(client: reqwest::Client, issuer: Url) -> Self {
        Self { client, issuer }
    }

    fn discovery_url(&self) -> Url {
        let base = self.issuer.as_str().trim_end_matches('/');
        Url::parse(&format!("{base}/.well-known/openid-configuration"))
            .unwrap_or_else(|err| unreachable!("issuer url plus a fixed suffix: {err}"))
    }
}

#[async_trait]
impl JwksSource for HttpJwksSource {
    async fn discovery(&self) -> Result<DiscoveryDocument, HttpError> {
        let request = self
            .client
            .get(self.discovery_url())
            .build()
            .map_err(|err| HttpError::Permanent(err.to_string()))?;
        let response = postit_http::execute_traced(&self.client, request).await?;
        response
            .json::<DiscoveryDocument>()
            .await
            .map_err(HttpError::from)
    }

    async fn jwks(&self) -> Result<JwkSet, HttpError> {
        let discovery = self.discovery().await?;
        let request = self
            .client
            .get(&discovery.jwks_uri)
            .build()
            .map_err(|err| HttpError::Permanent(err.to_string()))?;
        let response = postit_http::execute_traced(&self.client, request).await?;
        response.json::<JwkSet>().await.map_err(HttpError::from)
    }
}

#[derive(Default)]
struct CacheState {
    jwks: Option<JwkSet>,
    fetched_at: Option<Instant>,
    last_kid_refetch: Option<Instant>,
}

/// Caches the JWKS from `source`, refreshing it every `refresh_interval`, and refetches
/// immediately on an unknown `kid` — but never more than once per minute, so a flood of
/// tokens with random `kid`s can't be used to hammer the `IdP`.
pub struct OidcDiscovery<S: JwksSource> {
    source: S,
    refresh_interval: Duration,
    state: Mutex<CacheState>,
}

const KID_REFETCH_MIN_INTERVAL: Duration = Duration::from_secs(60);

impl<S: JwksSource> OidcDiscovery<S> {
    #[must_use]
    pub fn new(source: S, refresh_interval: Duration) -> Self {
        Self {
            source,
            refresh_interval,
            state: Mutex::new(CacheState::default()),
        }
    }

    /// # Errors
    ///
    /// Returns [`HttpError`] if the initial fetch fails and nothing is cached yet.
    pub async fn jwks(&self) -> Result<JwkSet, HttpError> {
        let stale = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .fetched_at
                .is_none_or(|t| t.elapsed() >= self.refresh_interval)
        };
        if stale {
            self.refetch().await?;
        }
        self.cached_jwks()
    }

    /// Looks up the JWKS containing `kid`. If the currently cached set doesn't have it,
    /// refetches at most once per [`KID_REFETCH_MIN_INTERVAL`] and returns the result
    /// either way — the caller (the [`crate::verifier::Verifier`] wiring in
    /// `postit-server`, P6) treats a still-missing `kid` after this as
    /// [`crate::VerifyError::UnknownKid`].
    ///
    /// # Errors
    ///
    /// Returns [`HttpError`] if a needed fetch fails and nothing is cached yet.
    pub async fn jwks_for_kid(&self, kid: &str) -> Result<JwkSet, HttpError> {
        let jwks = self.jwks().await?;
        if has_kid(&jwks, kid) {
            return Ok(jwks);
        }

        let may_refetch = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .last_kid_refetch
                .is_none_or(|t| t.elapsed() >= KID_REFETCH_MIN_INTERVAL)
        };
        if !may_refetch {
            return Ok(jwks);
        }

        self.refetch().await?;
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.last_kid_refetch = Some(Instant::now());
        }
        self.cached_jwks()
    }

    async fn refetch(&self) -> Result<(), HttpError> {
        let jwks = self.source.jwks().await?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.jwks = Some(jwks);
        state.fetched_at = Some(Instant::now());
        Ok(())
    }

    fn cached_jwks(&self) -> Result<JwkSet, HttpError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .jwks
            .clone()
            .ok_or_else(|| HttpError::Permanent("jwks not loaded".to_string()))
    }
}

fn has_kid(jwks: &JwkSet, kid: &str) -> bool {
    jwks.keys
        .iter()
        .any(|k| k.common.key_id.as_deref() == Some(kid))
}
