//! The account service: the one entry point for connecting, listing, checking, and
//! disconnecting accounts.

use std::sync::Arc;
use std::time::Duration;

use postit_config::AccountsSettings;
use postit_core::{
    AgentContext, Clock, CredentialCheckContext, GrantId, HttpHandles, IdGenerator, LinkSigner,
    MediaHost, MediaStore, PlatformId, UserId,
};
use postit_vault::Vault;
use serde_json::Value;
use sqlx::PgPool;
use url::Url;

use crate::{AccountError, PgCredentialStore, PlatformCatalog, ReturnToAllowlist};

/// How long the OAuth callback may spend on the code exchange, the agent's post-exchange hook,
/// and discovery together before it gives up and redirects with an error.
pub const CALLBACK_DEADLINE: Duration = Duration::from_secs(60);

/// What the service needs from the rest of the server.
pub struct AccountServiceDeps {
    /// The database.
    pub pool: PgPool,
    /// Seals and opens credentials.
    pub vault: Vault,
    /// The compiled-in agents and their deployment config.
    pub catalog: Arc<PlatformCatalog>,
    /// HTTP clients handed to agents; the OAuth engine sends token requests through the
    /// standard client in `default`.
    pub http: HttpHandles,
    /// The API's externally visible base URL (`server.public_url`), which OAuth redirect URIs
    /// are built on.
    pub public_url: Url,
    /// Where an OAuth connection may send the browser back to.
    pub return_to: ReturnToAllowlist,
    /// The OAuth callback's deadline for the exchange, the post-exchange hook, and discovery
    /// ([`CALLBACK_DEADLINE`] in a server).
    pub callback_deadline: Duration,
    /// Streams stored media to agents.
    pub media_store: Arc<dyn MediaStore>,
    /// Makes stored media reachable by URL.
    pub media_host: Arc<dyn MediaHost>,
    /// Builds tracked links for direct sends.
    pub link_signer: Arc<dyn LinkSigner>,
    /// The time source.
    pub clock: Arc<dyn Clock>,
    /// New row IDs.
    pub ids: Arc<dyn IdGenerator>,
    /// CSV import limits.
    pub settings: AccountsSettings,
}

/// Connects, lists, checks, and disconnects accounts. Cheap to clone.
#[derive(Clone)]
pub struct AccountService {
    pub(crate) deps: Arc<AccountServiceDeps>,
}

impl AccountService {
    /// A service over `deps`.
    #[must_use]
    pub fn new(deps: AccountServiceDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// The context `verify_credentials` runs in: no grant exists yet.
    pub(crate) fn check_context(&self, config: Value) -> CredentialCheckContext {
        CredentialCheckContext {
            config,
            http: self.deps.http.clone(),
            clock: Arc::clone(&self.deps.clock),
        }
    }

    /// The context a client bound to `grant` runs in.
    pub(crate) fn agent_context(
        &self,
        owner: UserId,
        grant: GrantId,
        config: Value,
    ) -> AgentContext {
        AgentContext {
            config,
            http: self.deps.http.clone(),
            credentials: Arc::new(PgCredentialStore::new(
                self.deps.pool.clone(),
                self.deps.vault.clone(),
                owner,
                grant,
            )),
            media_store: Arc::clone(&self.deps.media_store),
            media_host: Arc::clone(&self.deps.media_host),
            link_signer: Arc::clone(&self.deps.link_signer),
            clock: Arc::clone(&self.deps.clock),
        }
    }

    /// The platform's agent and config, read on a short-lived connection that is released
    /// before any platform call.
    pub(crate) async fn resolve(
        &self,
        platform: &PlatformId,
    ) -> Result<crate::ResolvedPlatform<'_>, AccountError> {
        let mut conn = self.deps.pool.acquire().await?;
        self.deps.catalog.resolve(&mut conn, platform).await
    }
}
