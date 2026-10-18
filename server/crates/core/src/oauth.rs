//! What the shared OAuth engine needs to know about a platform.

use std::time::Duration;

use url::Url;

/// Whether the authorization code flow uses PKCE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pkce {
    /// PKCE with the S256 challenge method.
    S256,
    /// No PKCE.
    None,
}

/// How the client authenticates at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// HTTP Basic with the client ID and secret.
    Basic,
    /// Client ID and secret in the request body.
    PostBody,
    /// Public client: the ID only.
    None,
}

/// Whether the platform issues refresh tokens and whether it rotates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPolicy {
    /// The platform issues refresh tokens.
    pub issued: bool,
    /// Each refresh returns a new refresh token.
    pub rotates: bool,
}

/// The endpoints and quirks of one platform's OAuth flow.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OAuthProviderSpec {
    /// Where the user is sent to approve access.
    pub authorize_url: Url,
    /// Where codes and refresh tokens are exchanged.
    pub token_url: Url,
    /// Where tokens are revoked, when the platform supports it.
    pub revoke_url: Option<Url>,
    /// Scopes to request.
    pub scopes: Vec<String>,
    /// What joins scopes in the request.
    pub scope_separator: String,
    /// PKCE use.
    pub pkce: Pkce,
    /// Client authentication at the token endpoint.
    pub client_auth: ClientAuth,
    /// Extra query parameters on the authorize URL.
    pub extra_authorize_params: Vec<(String, String)>,
    /// Refresh token behavior.
    pub refresh: RefreshPolicy,
    /// Access token lifetime assumed when the token response omits `expires_in`.
    pub default_access_ttl: Option<Duration>,
}

impl OAuthProviderSpec {
    /// A spec with the common defaults: S256 PKCE, Basic client auth, refresh tokens issued.
    #[must_use]
    pub fn new(authorize_url: Url, token_url: Url) -> Self {
        Self {
            authorize_url,
            token_url,
            revoke_url: None,
            scopes: Vec::new(),
            scope_separator: " ".to_owned(),
            pkce: Pkce::S256,
            client_auth: ClientAuth::Basic,
            extra_authorize_params: Vec::new(),
            refresh: RefreshPolicy {
                issued: true,
                rotates: false,
            },
            default_access_ttl: None,
        }
    }

    /// Sets the revocation endpoint.
    #[must_use]
    pub fn with_revoke_url(mut self, url: Url) -> Self {
        self.revoke_url = Some(url);
        self
    }

    /// Sets the scopes to request.
    #[must_use]
    pub fn with_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the string that joins scopes.
    #[must_use]
    pub fn with_scope_separator(mut self, separator: impl Into<String>) -> Self {
        self.scope_separator = separator.into();
        self
    }

    /// Sets PKCE use.
    #[must_use]
    pub fn with_pkce(mut self, pkce: Pkce) -> Self {
        self.pkce = pkce;
        self
    }

    /// Sets client authentication.
    #[must_use]
    pub fn with_client_auth(mut self, client_auth: ClientAuth) -> Self {
        self.client_auth = client_auth;
        self
    }

    /// Adds a query parameter to the authorize URL.
    #[must_use]
    pub fn with_extra_authorize_param(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.extra_authorize_params
            .push((name.into(), value.into()));
        self
    }

    /// Sets the refresh token policy.
    #[must_use]
    pub fn with_refresh(mut self, refresh: RefreshPolicy) -> Self {
        self.refresh = refresh;
        self
    }

    /// Sets the access token lifetime assumed when the response omits one.
    #[must_use]
    pub fn with_default_access_ttl(mut self, ttl: Duration) -> Self {
        self.default_access_ttl = Some(ttl);
        self
    }
}
