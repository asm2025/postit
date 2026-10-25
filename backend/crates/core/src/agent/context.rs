//! What the engine hands an agent: HTTP clients, credentials, media access, and the clock.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use url::Url;

use super::capability::DirectRecipient;
use crate::{AgentError, Clock, CredentialFields, DirectSendId, MediaHost, MediaStore};

/// An HTTP client owned by `postit-http`. Core cannot name the concrete type, so agents get it
/// through this handle and `postit-agent-kit` downcasts it.
pub trait HttpHandle: Send + Sync + 'static {
    /// The concrete handle, for downcasting by `postit-agent-kit`.
    fn as_any(&self) -> &dyn Any;
}

/// The two HTTP clients an agent may use.
#[derive(Clone)]
pub struct HttpHandles {
    /// For the platform's own API hosts.
    pub default: Arc<dyn HttpHandle>,
    /// For hosts the user supplied (a Mastodon instance, an SMTP relay). It applies the
    /// outbound address policy, so it refuses private and loopback addresses.
    pub user_hosts: Arc<dyn HttpHandle>,
}

/// OAuth tokens for one grant.
#[derive(Clone)]
pub struct OAuthTokens {
    /// The bearer token.
    pub access_token: SecretString,
    /// Used to get a new access token.
    pub refresh_token: Option<SecretString>,
    /// When the access token expires, when the platform says.
    pub expires_at: Option<DateTime<Utc>>,
    /// The scopes the grant carries.
    pub scopes: Vec<String>,
}

impl fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// What an account connects with.
#[derive(Debug, Clone)]
pub enum Credentials {
    /// OAuth tokens.
    OAuth(OAuthTokens),
    /// Fields the user typed in, such as a bot token or an app password.
    Fields(CredentialFields),
}

/// Reads and updates one grant's credentials. The engine implements it over the vault.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// The current credentials.
    ///
    /// # Errors
    /// [`AgentError::Authentication`] when the grant has none (the user must reconnect).
    async fn credentials(&self) -> Result<Credentials, AgentError>;

    /// Saves tokens the agent just refreshed, so the next call uses them.
    ///
    /// # Errors
    /// An [`AgentError`] when the vault write fails.
    async fn store_refreshed(&self, tokens: OAuthTokens) -> Result<(), AgentError>;
}

/// Builds the tracked links in an email so opens, clicks, and unsubscribes can be counted.
pub trait LinkSigner: Send + Sync {
    /// The one-click unsubscribe link for a recipient.
    fn unsubscribe_url(&self, recipient: &DirectRecipient) -> Url;
    /// A link that records the click, then redirects to `destination`.
    fn click_url(&self, send_id: DirectSendId, destination: &Url) -> Url;
    /// The tracking pixel's URL.
    fn open_pixel_url(&self, send_id: DirectSendId) -> Url;
}

/// Everything an agent client needs. One context per grant.
#[derive(Clone)]
pub struct AgentContext {
    /// The agent's stored configuration, shaped by `config_schema`.
    pub config: serde_json::Value,
    /// HTTP clients.
    pub http: HttpHandles,
    /// This grant's credentials.
    pub credentials: Arc<dyn CredentialStore>,
    /// Streams stored media.
    pub media_store: Arc<dyn MediaStore>,
    /// Makes stored media reachable by URL.
    pub media_host: Arc<dyn MediaHost>,
    /// Tracked links for direct sends.
    pub link_signer: Arc<dyn LinkSigner>,
    /// The time source.
    pub clock: Arc<dyn Clock>,
}

/// What `verify_credentials` gets: no stored grant exists yet.
#[derive(Clone)]
pub struct CredentialCheckContext {
    /// The agent's stored configuration.
    pub config: serde_json::Value,
    /// HTTP clients.
    pub http: HttpHandles,
    /// The time source.
    pub clock: Arc<dyn Clock>,
}

/// What `handle_event` gets: an inbound webhook has no grant either.
#[derive(Clone)]
pub struct EventContext {
    /// The agent's stored configuration.
    pub config: serde_json::Value,
    /// The time source.
    pub clock: Arc<dyn Clock>,
}
