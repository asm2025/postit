//! Connecting accounts through OAuth: start an authorization, handle the provider's callback,
//! and read the pending result the callback leaves for the user to confirm.
//!
//! Nothing here logs, returns, or redirects with a secret: tokens, codes, the PKCE verifier,
//! `state`, and client secrets stay sealed or in memory. `connect_id` appears only in the
//! success redirect. No pooled database connection is held during a provider call.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use postit_core::{
    AccountRef, AccountToken, AgentContext, AgentError, ConnectMethod, CredentialStore,
    Credentials, DiscoveredAccount, ExchangeContext, GrantExchange, OAuthProviderSpec,
    OAuthStateId, OAuthTokens, PlatformId,
};
use postit_data::oauth_states::{NewOAuthState, OAuthState, OAuthStateRepo};
use postit_data::pending_connections::PendingConnectionRepo;
use postit_data::{Capability, DataError, OwnerScope};
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::connect::{ConnectId, hash_of, random_secret};
use crate::engine::{self, ClientCredentials, Pkce, TokenError};
use crate::error::agent_error_name;
use crate::manage::ConnectKind;
use crate::{AccountError, AccountService};

/// How long a started authorization, and then its pending result, stays valid.
const STATE_TTL: Duration = Duration::minutes(10);

/// The sealed columns of an `oauth_states` row.
const PKCE_COLUMN: &str = "pkce_verifier";
pub(crate) const TOKENS_COLUMN: &str = "grant_tokens";

/// A started authorization: where to send the user, and until when the callback is accepted.
pub struct OAuthStart {
    /// The provider's authorize URL. It carries the `state`, so it is handed to the user only.
    pub authorize_url: Url,
    /// When the state expires.
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for OAuthStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthStart")
            .field("authorize_url", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The query of the provider's redirect to the callback.
pub struct CallbackParams {
    /// The authorization code.
    pub code: Option<SecretString>,
    /// The `state` postit sent.
    pub state: Option<String>,
    /// The provider's OAuth `error`, when the user denied or the provider failed.
    pub error: Option<String>,
}

impl fmt::Debug for CallbackParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackParams")
            .field("has_code", &self.code.is_some())
            .field("has_state", &self.state.is_some())
            .field("has_error", &self.error.is_some())
            .finish()
    }
}

/// Why a callback failed: the `code` of its error redirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackCode {
    /// The user denied the authorization.
    AccessDenied,
    /// The provider redirected with another error, or with no code.
    ProviderError,
    /// The `state` is unknown, used, expired, or for another platform.
    StateInvalid,
    /// The code exchange failed or ran out of time.
    ExchangeFailed,
    /// The user did not grant every scope the platform needs.
    ScopeMissing,
    /// The agent's post-exchange hook or account discovery failed or ran out of time.
    DiscoveryFailed,
    /// The authorization reaches no account.
    NoAccounts,
    /// The platform was switched off, lost its deployment config, or rejected the client.
    PlatformUnavailable,
}

impl CallbackCode {
    /// The value of the redirect's `code` parameter.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccessDenied => "access_denied",
            Self::ProviderError => "provider_error",
            Self::StateInvalid => "state_invalid",
            Self::ExchangeFailed => "exchange_failed",
            Self::ScopeMissing => "scope_missing",
            Self::DiscoveryFailed => "discovery_failed",
            Self::NoAccounts => "no_accounts",
            Self::PlatformUnavailable => "platform_unavailable",
        }
    }
}

/// A pending connection, read without consuming it.
#[derive(Debug, Clone)]
pub struct PendingView {
    /// The platform.
    pub platform: PlatformId,
    /// Whether it came from pasted credentials or an OAuth sign-in.
    pub connect: ConnectKind,
    /// When it can no longer be confirmed.
    pub expires_at: DateTime<Utc>,
    /// The accounts it reaches.
    pub discovered: Vec<DiscoveredAccount>,
}

/// The tokens of an exchange as sealed into `oauth_states.grant_tokens`: the agent's
/// [`GrantExchange`] without the discovered accounts.
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredExchange {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub remote_user_id: Option<String>,
    pub reconnect_by: Option<DateTime<Utc>>,
    pub account_tokens: BTreeMap<String, StoredAccountToken>,
}

/// One account's token inside a [`StoredExchange`].
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredAccountToken {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl fmt::Debug for StoredExchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredExchange")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("reconnect_by", &self.reconnect_by)
            .finish_non_exhaustive()
    }
}

impl StoredExchange {
    /// The exchanged tokens alone, before the agent's hook ran.
    fn from_tokens(tokens: &OAuthTokens) -> Self {
        let mut stored = Self {
            access_token: String::new(),
            refresh_token: None,
            expires_at: None,
            scopes: Vec::new(),
            remote_user_id: None,
            reconnect_by: None,
            account_tokens: BTreeMap::new(),
        };
        stored.set_tokens(tokens);
        stored
    }

    /// Everything the agent's hook returned.
    fn from_grant(grant: &GrantExchange) -> Self {
        let mut stored = Self::from_tokens(&grant.tokens);
        stored.remote_user_id.clone_from(&grant.remote_user_id);
        stored.reconnect_by = grant.reconnect_by;
        stored.account_tokens = grant
            .account_tokens
            .iter()
            .map(|(id, token)| {
                (
                    id.clone(),
                    StoredAccountToken {
                        access_token: token.access_token.expose_secret().to_owned(),
                        expires_at: token.expires_at,
                    },
                )
            })
            .collect();
        stored
    }

    fn set_tokens(&mut self, tokens: &OAuthTokens) {
        tokens
            .access_token
            .expose_secret()
            .clone_into(&mut self.access_token);
        self.refresh_token = tokens
            .refresh_token
            .as_ref()
            .map(|token| token.expose_secret().to_owned());
        self.expires_at = tokens.expires_at;
        self.scopes.clone_from(&tokens.scopes);
    }

    /// The JSON value that is sealed.
    fn to_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}

/// The credential store discovery runs against: the exchange's tokens, in memory only.
struct ExchangedCredentials {
    tokens: Mutex<OAuthTokens>,
    account_tokens: BTreeMap<String, AccountToken>,
}

impl ExchangedCredentials {
    fn new(grant: &GrantExchange) -> Self {
        Self {
            tokens: Mutex::new(grant.tokens.clone()),
            account_tokens: grant.account_tokens.clone(),
        }
    }

    fn current(&self) -> OAuthTokens {
        self.tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl CredentialStore for ExchangedCredentials {
    async fn credentials(&self) -> Result<Credentials, AgentError> {
        Ok(Credentials::OAuth(self.current()))
    }

    async fn store_refreshed(&self, tokens: OAuthTokens) -> Result<(), AgentError> {
        *self.tokens.lock().unwrap_or_else(PoisonError::into_inner) = tokens;
        Ok(())
    }

    async fn account_token(
        &self,
        account: &AccountRef,
    ) -> Result<Option<AccountToken>, AgentError> {
        Ok(self.account_tokens.get(&account.remote_id).cloned())
    }
}

/// What the callback needs from the platform before it calls the provider.
struct Prepared<'a> {
    agent: &'a dyn postit_core::Agent,
    config: Value,
    spec: OAuthProviderSpec,
    client: ClientCredentials,
    verifier: Option<SecretString>,
}

/// How far the callback got: the tokens once the exchange succeeded, so a later failure or the
/// deadline discards them for the purge to revoke instead of dropping them.
#[derive(Default)]
struct Progress {
    exchanged: Option<StoredExchange>,
}

impl AccountService {
    /// Starts connecting accounts of an OAuth platform: stores the hashed `state` and the
    /// sealed PKCE verifier, and returns the provider's authorize URL.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, a platform error from the
    /// catalog, [`AccountError::ConnectMethodUnsupported`] for a credentials platform,
    /// [`AccountError::PlatformUnavailable`] when the client ID or secret is missing, and
    /// [`AccountError::ReturnToInvalid`] when `return_to` is not exactly an allowed URL.
    pub async fn start_oauth(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        return_to: &str,
    ) -> Result<OAuthStart, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let resolved = self.resolve(platform).await?;
        let ConnectMethod::OAuth(spec) = &resolved.agent.descriptor().connect else {
            return Err(AccountError::ConnectMethodUnsupported);
        };
        let client = ClientCredentials::from_config(&resolved.config)?;
        if !self.deps.return_to.allows(return_to) {
            return Err(AccountError::ReturnToInvalid);
        }

        let id = OAuthStateId::from(self.deps.ids.generate());
        let state = random_secret();
        let pkce = match spec.pkce {
            postit_core::Pkce::S256 => Some(Pkce::generate()),
            postit_core::Pkce::None => None,
        };
        let pkce_verifier = pkce
            .as_ref()
            .map(|pkce| {
                self.deps.vault.seal(
                    &json!(pkce.verifier.expose_secret()),
                    &OAuthStateRepo::seal_context(id, PKCE_COLUMN),
                )
            })
            .transpose()?;
        let expires_at = self.deps.clock.now() + STATE_TTL;
        let mut conn = self.deps.pool.acquire().await?;
        OAuthStateRepo::insert(
            &mut conn,
            NewOAuthState {
                id,
                state_hash: hash_of(&state),
                user: scope.actor,
                platform: platform.clone(),
                pkce_verifier,
                return_to: return_to.to_owned(),
                expires_at,
            },
        )
        .await?;
        drop(conn);

        let redirect_uri = engine::redirect_uri(&self.deps.public_url, platform);
        let authorize_url =
            engine::authorize_url(spec, &client, &redirect_uri, &state, pkce.as_ref());
        Ok(OAuthStart {
            authorize_url,
            expires_at,
        })
    }

    /// Handles the provider's redirect and returns where to send the browser (a 303).
    ///
    /// Consumes the state, exchanges the code, runs the agent's post-exchange hook, discovers
    /// the accounts, and stores them as a pending result for the user to confirm. A `state`
    /// that is unknown, used, expired, or for another platform lands on the web app's
    /// `/oauth-done` with `state_invalid`, since no trustworthy `return_to` exists; anything
    /// else goes to the state's `return_to`. A failure before the tokens exist deletes the
    /// row; a failure after discards it, so the purge revokes the tokens.
    pub async fn oauth_callback(&self, platform: &str, params: CallbackParams) -> Url {
        let web = self.deps.return_to.web().clone();
        let Some(row) = self.take_state(platform, params.state.as_deref()).await else {
            tracing::warn!(platform, code = "state_invalid", "oauth callback refused");
            return error_redirect(&web, CallbackCode::StateInvalid);
        };
        // `return_to` passed the allowlist when the state was stored.
        let return_to = Url::parse(&row.return_to).unwrap_or(web);
        match self.complete(&row, params).await {
            Ok(connect_id) => with_query(
                &return_to,
                &[("result", "ok"), ("connect_id", connect_id.as_str())],
            ),
            Err(code) => {
                tracing::warn!(
                    platform = %row.platform,
                    code = code.as_str(),
                    "oauth callback failed"
                );
                error_redirect(&return_to, code)
            }
        }
    }

    /// Reads a pending connection, OAuth or pasted credentials, without consuming it.
    ///
    /// Only the user who started it may read it: anyone else's read ends it (a credential
    /// connection is deleted, an OAuth result discarded for the purge to revoke) and answers
    /// `NotFound`, as a confirm does.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, and [`AccountError::NotFound`]
    /// for an unknown, expired, or foreign `connect_id`.
    pub async fn pending(
        &self,
        scope: &OwnerScope,
        connect_id: &str,
    ) -> Result<PendingView, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let hash = hash_of(connect_id);
        let now = self.deps.clock.now();
        let mut conn = self.deps.pool.acquire().await?;

        if let Some(pending) = PendingConnectionRepo::find(&mut conn, &hash).await? {
            if pending.owner != scope.owner {
                // A credential result holds no OAuth tokens: deleting it is enough.
                PendingConnectionRepo::take(&mut conn, &hash).await?;
                return Err(AccountError::NotFound);
            }
            if pending.expires_at <= now {
                return Err(AccountError::NotFound);
            }
            return Ok(PendingView {
                platform: pending.platform,
                connect: ConnectKind::Credentials,
                expires_at: pending.expires_at,
                discovered: stored_accounts(pending.discovered)?,
            });
        }

        let Some(result) = OAuthStateRepo::find_result(&mut conn, &hash).await? else {
            return Err(AccountError::NotFound);
        };
        if result.user != scope.owner {
            OAuthStateRepo::discard(&mut conn, result.id, now).await?;
            return Err(AccountError::NotFound);
        }
        if result.expires_at <= now {
            return Err(AccountError::NotFound);
        }
        let discovered = result.discovered.ok_or(AccountError::NotFound)?;
        Ok(PendingView {
            platform: result.platform,
            connect: ConnectKind::OAuth,
            expires_at: result.expires_at,
            discovered: stored_accounts(discovered)?,
        })
    }

    /// Consumes the state named by `state` (step 1). `None` when it is missing, unknown,
    /// already used, expired, or for another platform; a consumed row that is refused is
    /// deleted, since it holds no tokens.
    async fn take_state(&self, platform: &str, state: Option<&str>) -> Option<OAuthState> {
        let state = state?;
        let now = self.deps.clock.now();
        let mut conn = match self.deps.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::error!(error = %error, "oauth callback: database unavailable");
                return None;
            }
        };
        let row = match OAuthStateRepo::consume_state(&mut conn, &hash_of(state), now).await {
            Ok(row) => row?,
            Err(error) => {
                tracing::error!(error = %error, "oauth callback: consuming the state failed");
                return None;
            }
        };
        if row.platform.as_str() != platform || row.expires_at <= now {
            if let Err(error) = OAuthStateRepo::delete(&mut conn, row.id).await {
                tracing::error!(error = %error, "oauth callback: deleting a refused state failed");
            }
            return None;
        }
        Some(row)
    }

    /// Steps 2 to 6 for a consumed, valid state: the `connect_id` of the stored result, or
    /// the code to redirect with after the row was deleted or discarded.
    async fn complete(
        &self,
        row: &OAuthState,
        params: CallbackParams,
    ) -> Result<ConnectId, CallbackCode> {
        // Before the exchange no tokens exist, so a failure deletes the row.
        let ready = match provider_code(params) {
            Ok(code) => self.prepare(row).await.map(|prepared| (code, prepared)),
            Err(code) => Err(code),
        };
        let (code, prepared) = match ready {
            Ok(ready) => ready,
            Err(code) => {
                self.delete_state(row.id).await;
                return Err(code);
            }
        };

        let mut progress = Progress::default();
        let outcome = tokio::time::timeout(
            self.deps.callback_deadline,
            self.exchange_and_discover(row, &prepared, &code, &mut progress),
        )
        .await
        .unwrap_or_else(|_| {
            tracing::warn!(platform = %row.platform, "oauth callback ran past its deadline");
            Err(if progress.exchanged.is_some() {
                CallbackCode::DiscoveryFailed
            } else {
                CallbackCode::ExchangeFailed
            })
        });
        let Some(exchanged) = progress.exchanged else {
            // Nothing was exchanged, so no tokens exist to keep.
            self.delete_state(row.id).await;
            return Err(outcome.err().unwrap_or(CallbackCode::ExchangeFailed));
        };
        match outcome {
            Ok(discovered) => self.store_result(row, &exchanged, &discovered).await,
            Err(code) => {
                self.store_discarded(row, &exchanged).await;
                Err(code)
            }
        }
    }

    /// The platform's agent, spec, client, and the opened PKCE verifier. The platform must
    /// still be available: an admin may have switched it off mid-flow.
    async fn prepare(&self, row: &OAuthState) -> Result<Prepared<'_>, CallbackCode> {
        let resolved = self
            .resolve(&row.platform)
            .await
            .map_err(|_| CallbackCode::PlatformUnavailable)?;
        let ConnectMethod::OAuth(spec) = &resolved.agent.descriptor().connect else {
            return Err(CallbackCode::PlatformUnavailable);
        };
        let client = ClientCredentials::from_config(&resolved.config)
            .map_err(|_| CallbackCode::PlatformUnavailable)?;
        let verifier = match &row.pkce_verifier {
            None => None,
            Some(sealed) => {
                let opened: Value = self
                    .deps
                    .vault
                    .open(sealed, &OAuthStateRepo::seal_context(row.id, PKCE_COLUMN))
                    .map_err(|_| CallbackCode::ExchangeFailed)?;
                let Value::String(verifier) = opened else {
                    return Err(CallbackCode::ExchangeFailed);
                };
                Some(SecretString::from(verifier))
            }
        };
        Ok(Prepared {
            agent: resolved.agent,
            config: resolved.config,
            spec: spec.clone(),
            client,
            verifier,
        })
    }

    /// Steps 3 to 5, run under the callback deadline: exchange the code, run the agent's
    /// hook, check the scopes, and discover the accounts. Records the tokens in `progress` as
    /// soon as they exist.
    async fn exchange_and_discover(
        &self,
        row: &OAuthState,
        prepared: &Prepared<'_>,
        code: &SecretString,
        progress: &mut Progress,
    ) -> Result<Vec<DiscoveredAccount>, CallbackCode> {
        let redirect_uri = engine::redirect_uri(&self.deps.public_url, &row.platform);
        let exchange = engine::exchange_code(
            &self.deps.http,
            &prepared.spec,
            &prepared.client,
            &redirect_uri,
            code,
            prepared.verifier.as_ref(),
            self.deps.clock.now(),
        )
        .await
        .map_err(|error| match error {
            TokenError::InvalidClient => CallbackCode::PlatformUnavailable,
            _ => CallbackCode::ExchangeFailed,
        })?;
        progress.exchanged = Some(StoredExchange::from_tokens(&exchange.tokens));
        let missing = engine::missing_scopes(&prepared.spec, &exchange.tokens.scopes);

        // The hook runs even when scopes are missing: it names the remote user, without which
        // the purge could not tell whether revoking the tokens is safe.
        let context = ExchangeContext {
            config: prepared.config.clone(),
            http: self.deps.http.clone(),
            clock: Arc::clone(&self.deps.clock),
        };
        let grant = prepared
            .agent
            .after_exchange(&context, exchange)
            .await
            .map_err(|error| {
                tracing::warn!(
                    error = agent_error_name(&error),
                    "oauth post-exchange hook failed"
                );
                if missing.is_empty() {
                    CallbackCode::DiscoveryFailed
                } else {
                    CallbackCode::ScopeMissing
                }
            })?;
        progress.exchanged = Some(StoredExchange::from_grant(&grant));
        if !missing.is_empty() {
            return Err(CallbackCode::ScopeMissing);
        }

        let credentials = Arc::new(ExchangedCredentials::new(&grant));
        let context = AgentContext {
            config: prepared.config.clone(),
            http: self.deps.http.clone(),
            credentials: Arc::clone(&credentials) as Arc<dyn CredentialStore>,
            media_store: Arc::clone(&self.deps.media_store),
            media_host: Arc::clone(&self.deps.media_host),
            link_signer: Arc::clone(&self.deps.link_signer),
            clock: Arc::clone(&self.deps.clock),
        };
        let discovered = match prepared.agent.create_client(context) {
            Ok(client) => client.discover_accounts().await,
            Err(error) => Err(error),
        };
        // An agent that refreshed during discovery saved the new tokens in the store.
        if let Some(stored) = progress.exchanged.as_mut() {
            stored.set_tokens(&credentials.current());
        }
        let discovered = discovered.map_err(|error| {
            tracing::warn!(
                error = agent_error_name(&error),
                "oauth account discovery failed"
            );
            CallbackCode::DiscoveryFailed
        })?;
        if discovered.accounts.is_empty() {
            return Err(CallbackCode::NoAccounts);
        }
        Ok(discovered.accounts)
    }

    /// Step 6: stores the pending result. A failure to store it discards the tokens instead.
    async fn store_result(
        &self,
        row: &OAuthState,
        exchanged: &StoredExchange,
        discovered: &[DiscoveredAccount],
    ) -> Result<ConnectId, CallbackCode> {
        let connect_id = ConnectId::generate();
        let stored = async {
            let sealed = self.seal_tokens(row.id, exchanged)?;
            let discovered = serde_json::to_value(discovered)
                .map_err(|_| AccountError::Data(DataError::Conflict("discovered".to_owned())))?;
            let mut conn = self.deps.pool.acquire().await?;
            OAuthStateRepo::store_result(
                &mut conn,
                row.id,
                &connect_id.hash(),
                &sealed,
                &discovered,
                exchanged.remote_user_id.as_deref(),
            )
            .await?;
            Ok::<_, AccountError>(())
        };
        match stored.await {
            Ok(()) => Ok(connect_id),
            Err(error) => {
                tracing::error!(error = %error, "oauth callback: storing the result failed");
                self.store_discarded(row, exchanged).await;
                Err(CallbackCode::DiscoveryFailed)
            }
        }
    }

    /// Keeps the tokens of a failed callback on a discarded row, for the purge to revoke.
    async fn store_discarded(&self, row: &OAuthState, exchanged: &StoredExchange) {
        let stored = async {
            let sealed = self.seal_tokens(row.id, exchanged)?;
            let mut conn = self.deps.pool.acquire().await?;
            OAuthStateRepo::store_discarded(
                &mut conn,
                row.id,
                &sealed,
                exchanged.remote_user_id.as_deref(),
                self.deps.clock.now(),
            )
            .await?;
            Ok::<_, AccountError>(())
        };
        if let Err(error) = stored.await {
            tracing::error!(error = %error, "oauth callback: discarding the result failed");
        }
    }

    fn seal_tokens(
        &self,
        id: OAuthStateId,
        exchanged: &StoredExchange,
    ) -> Result<postit_vault::Sealed<Value>, AccountError> {
        let value = exchanged
            .to_value()
            .map_err(|_| AccountError::Data(DataError::Conflict("grant tokens".to_owned())))?;
        Ok(self
            .deps
            .vault
            .seal(&value, &OAuthStateRepo::seal_context(id, TOKENS_COLUMN))?)
    }

    /// Deletes a row that holds no tokens.
    async fn delete_state(&self, id: OAuthStateId) {
        let deleted = async {
            let mut conn = self.deps.pool.acquire().await?;
            OAuthStateRepo::delete(&mut conn, id).await?;
            Ok::<_, AccountError>(())
        };
        if let Err(error) = deleted.await {
            tracing::error!(error = %error, "oauth callback: deleting the state failed");
        }
    }
}

/// Step 2: the authorization code, unless the provider redirected with an error (any `error`
/// wins over a `code`) or with no code at all.
fn provider_code(params: CallbackParams) -> Result<SecretString, CallbackCode> {
    match (params.error.as_deref(), params.code) {
        (Some("access_denied"), _) => Err(CallbackCode::AccessDenied),
        (Some(_), _) | (None, None) => Err(CallbackCode::ProviderError),
        (None, Some(code)) => Ok(code),
    }
}

/// The accounts a pending row stored.
pub(crate) fn stored_accounts(value: Value) -> Result<Vec<DiscoveredAccount>, AccountError> {
    serde_json::from_value(value)
        .map_err(|_| AccountError::Data(DataError::Conflict("stored accounts".to_owned())))
}

fn error_redirect(base: &Url, code: CallbackCode) -> Url {
    with_query(base, &[("result", "error"), ("code", code.as_str())])
}

/// `base` with exactly `pairs` as its query.
fn with_query(base: &Url, pairs: &[(&str, &str)]) -> Url {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut().extend_pairs(pairs);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_codes_are_stable() {
        let codes = [
            (CallbackCode::AccessDenied, "access_denied"),
            (CallbackCode::ProviderError, "provider_error"),
            (CallbackCode::StateInvalid, "state_invalid"),
            (CallbackCode::ExchangeFailed, "exchange_failed"),
            (CallbackCode::ScopeMissing, "scope_missing"),
            (CallbackCode::DiscoveryFailed, "discovery_failed"),
            (CallbackCode::NoAccounts, "no_accounts"),
            (CallbackCode::PlatformUnavailable, "platform_unavailable"),
        ];
        for (code, text) in codes {
            assert_eq!(code.as_str(), text);
        }
    }

    #[test]
    fn redirects_carry_only_their_pairs() {
        let base = Url::parse("postit-dev://oauth-done").unwrap_or_else(|_| unreachable!());
        assert_eq!(
            error_redirect(&base, CallbackCode::NoAccounts).as_str(),
            "postit-dev://oauth-done?result=error&code=no_accounts"
        );
    }
}
