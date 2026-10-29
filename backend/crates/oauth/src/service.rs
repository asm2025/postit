//! The account service: the one entry point for connecting, listing, checking, and
//! disconnecting accounts.

use std::sync::Arc;
use std::time::Duration;

use postit_config::AccountsSettings;
use postit_core::{
    AccountId, AccountRef, AgentClient, AgentContext, AgentDescriptor, Clock,
    CredentialCheckContext, CredentialStore, GrantId, HttpHandles, IdGenerator, LinkSigner,
    MediaHost, MediaStore, PlatformId, UserId,
};
use postit_data::OwnerScope;
use postit_data::accounts::{AccountRepo, AccountStatus};
use postit_vault::Vault;
use serde_json::Value;
use sqlx::PgPool;
use url::Url;

use crate::{AccountError, DisconnectHook, PgCredentialStore, PlatformCatalog, ReturnToAllowlist};

/// How long the OAuth callback may spend on the code exchange, the agent's post-exchange hook,
/// and discovery together before it gives up and redirects with an error.
pub const CALLBACK_DEADLINE: Duration = Duration::from_secs(60);

/// What the service needs from the rest of the server.
#[derive(Clone)]
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
    /// The publishing engine's part of a disconnect; `None` where no engine runs (tests, or
    /// a build without publishing), which leaves native uploads on the platform.
    pub disconnect_hook: Option<Arc<dyn DisconnectHook>>,
}

/// A platform client bound to one connected account's grant, for the worker. Built without a
/// scope: the caller has already authorized the owner's work.
pub struct BoundClient {
    /// The agent's client, holding the grant's credential store.
    pub client: Box<dyn AgentClient>,
    /// The account the client acts for.
    pub account: AccountRef,
    /// What the account's agent declares.
    pub descriptor: AgentDescriptor,
    /// The credential grant the client is bound to.
    pub grant: GrantId,
    /// The grant's owner.
    pub owner: UserId,
    /// The platform's resolved `[platforms.<id>]` table, or an empty object; the engine reads
    /// its `quota` overrides. Holds secrets: never log or return it.
    pub config: Value,
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

    /// This service with `hook` run on every disconnect. The composition root builds the
    /// publishing engine over a service without one, then hands the API this one.
    #[must_use]
    pub fn with_disconnect_hook(&self, hook: Arc<dyn DisconnectHook>) -> Self {
        let mut deps = (*self.deps).clone();
        deps.disconnect_hook = Some(hook);
        Self::new(deps)
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
        let credentials = Arc::new(PgCredentialStore::new(self.clone(), owner, grant));
        self.agent_context_with(config, credentials)
    }

    /// The context a client runs in with `credentials` as its store.
    pub(crate) fn agent_context_with(
        &self,
        config: Value,
        credentials: Arc<dyn CredentialStore>,
    ) -> AgentContext {
        AgentContext {
            config,
            http: self.deps.http.clone(),
            credentials,
            media_store: Arc::clone(&self.deps.media_store),
            media_host: Arc::clone(&self.deps.media_host),
            link_signer: Arc::clone(&self.deps.link_signer),
            clock: Arc::clone(&self.deps.clock),
        }
    }

    /// A client for `account`, bound to its grant. System-side: no scope, the caller has
    /// already authorized `owner`'s work; the account must be the owner's.
    ///
    /// # Errors
    ///
    /// [`AccountError::NotFound`] for an account the owner does not have,
    /// [`AccountError::AccountDisconnected`], and the catalog's errors
    /// ([`AccountError::PlatformNotFound`], [`AccountError::PlatformUnavailable`],
    /// [`AccountError::PlatformDisabled`]) unchanged.
    pub async fn bound_client(
        &self,
        owner: UserId,
        account: AccountId,
    ) -> Result<BoundClient, AccountError> {
        let row = {
            let mut conn = self.deps.pool.acquire().await?;
            AccountRepo::get(&mut conn, &OwnerScope::own(owner), account).await?
        };
        let grant = match row.grant_id {
            Some(grant) if row.status != AccountStatus::Disconnected => grant,
            _ => return Err(AccountError::AccountDisconnected),
        };
        let resolved = self.resolve(&row.platform).await?;
        let config = resolved.config.clone();
        let client = resolved
            .agent
            .create_client(self.agent_context(owner, grant, resolved.config.clone()))
            .map_err(AccountError::from_agent)?;
        Ok(BoundClient {
            client,
            account: AccountRef {
                platform: row.platform,
                account,
                remote_id: row.remote_account_id,
            },
            descriptor: resolved.agent.descriptor().clone(),
            grant,
            owner,
            config,
        })
    }

    /// Whether accounts on `platform` can be used right now: compiled in, switched on, and
    /// configured. Reads the platform's `enabled` flag on every call. A platform this build
    /// has an agent for but no `platforms` row is treated as switched off, as the due sweep
    /// treats it.
    ///
    /// # Errors
    ///
    /// [`AccountError::Data`] on a database failure.
    pub async fn platform_availability(
        &self,
        platform: &PlatformId,
    ) -> Result<crate::Availability, AccountError> {
        let row = {
            let mut conn = self.deps.pool.acquire().await?;
            postit_data::platforms::PlatformRepo::get(&mut conn, platform).await?
        };
        let registry = self.deps.catalog.registry();
        Ok(match row {
            Some(row) => crate::catalog::availability(&row, registry, self.deps.catalog.configs()),
            None if registry.get(platform).is_some() => crate::Availability::Disabled,
            None => crate::Availability::NotCompiledIn,
        })
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
