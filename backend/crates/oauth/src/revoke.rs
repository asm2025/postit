//! Revoking at the provider: the one primitive every OAuth revoke goes through
//! ([`AccountService::revoke_tokens`]), the grant revoke used by disconnect and user deletion
//! ([`AccountService::revoke_grant`]), the revoke of a taken pending result, and the OAuth
//! user deletion step.
//!
//! Every revoke of OAuth tokens follows the revoke-skip rule: no provider call when the tokens
//! carry no remote user ID, or when that remote user is still in use (an `active` grant of any
//! owner, or a live pending result), because a provider that revokes per user and app pair
//! would end that other authorization too.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::{
    ConnectMethod, CredentialStore, GrantExchange, GrantId, OAuthTokens, PlatformId, UserId,
};
use postit_data::accounts::{Grant, GrantKind, GrantRepo};
use postit_data::oauth_states::{OAuthState, OAuthStateRepo, RemoteUserExcept};
use postit_identity::UserDeletionStep;
use postit_jobs::JobError;
use secrecy::SecretString;
use serde_json::Value;

use crate::AccountService;
use crate::engine::{self, ClientCredentials};
use crate::error::agent_error_name;
use crate::flow::{ExchangedCredentials, StoredExchange, TOKENS_COLUMN};

impl AccountService {
    /// Best effort: asks the provider to revoke `tokens`. When the spec has an RFC 7009
    /// endpoint the engine sends the refresh token (hint `refresh_token`), or the access token
    /// when there is none (hint `access_token`); otherwise the agent's `revoke()` runs over an
    /// in-memory store holding `tokens`. A failure is logged by its kind only. Whether the
    /// revoke should happen at all (the revoke-skip rule) is the caller's call.
    pub(crate) async fn revoke_tokens(
        &self,
        platform: &PlatformId,
        config: &Value,
        tokens: &OAuthTokens,
    ) {
        let Some(agent) = self.deps.catalog.registry().get(platform) else {
            tracing::warn!(%platform, "no agent to revoke the tokens with");
            return;
        };
        let ConnectMethod::OAuth(spec) = &agent.descriptor().connect else {
            tracing::warn!(%platform, "the platform does not connect through OAuth; nothing revoked");
            return;
        };
        if spec.revoke_url.is_some() {
            let Ok(client) = ClientCredentials::from_config(config) else {
                tracing::warn!(%platform, "no client credentials to revoke the tokens with");
                return;
            };
            let (token, hint) = match &tokens.refresh_token {
                Some(refresh) => (refresh, "refresh_token"),
                None => (&tokens.access_token, "access_token"),
            };
            if let Err(error) = engine::revoke(&self.deps.http, spec, &client, token, hint).await {
                tracing::warn!(%platform, %error, "revoking the tokens did not succeed");
            }
            return;
        }
        let store: Arc<dyn CredentialStore> = Arc::new(ExchangedCredentials::new(
            &GrantExchange::from_tokens(tokens.clone()),
        ));
        let context = self.agent_context_with(config.clone(), store);
        match agent.create_client(context) {
            Ok(client) => {
                if let Err(error) = client.revoke().await {
                    tracing::warn!(%platform, error = agent_error_name(&error), "revoking the tokens did not succeed");
                }
            }
            Err(error) => {
                tracing::warn!(%platform, error = agent_error_name(&error), "no client to revoke the tokens with");
            }
        }
    }

    /// Best effort, never an error: asks the platform to revoke `grant`, which belongs to
    /// `owner`. A credentials grant goes to the agent's `revoke()`. An OAuth grant follows the
    /// revoke-skip rule, and is revoked under the grant's refresh lock with the credential read
    /// inside that lock, so a refresh running at the same moment finishes first and the revoke
    /// sends the token it stored, not the one the provider retired. The caller deletes or marks
    /// the grant afterwards; this changes no row.
    pub async fn revoke_grant(&self, owner: UserId, grant: &Grant) {
        match grant.kind {
            GrantKind::Oauth => self.revoke_oauth_grant(grant.id).await,
            GrantKind::Credentials => {
                self.revoke_with_agent(owner, grant.id, &grant.platform)
                    .await;
            }
        }
    }

    /// The OAuth arm of [`AccountService::revoke_grant`].
    async fn revoke_oauth_grant(&self, id: GrantId) {
        // The transaction holds the grant's refresh lock across the provider call by design.
        // Nothing is written: dropping it at the end rolls back and releases the lock.
        let mut tx = match self.deps.pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(grant = %id, ?error, "no database to revoke the grant with");
                return;
            }
        };
        let grant = match GrantRepo::lock_for_refresh(&mut tx, id).await {
            Ok(Some((_, grant))) => grant,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(grant = %id, ?error, "could not lock the grant to revoke it");
                return;
            }
        };
        let platform = &grant.platform;
        let Some(remote) = grant.remote_user_id.as_deref() else {
            tracing::info!(%platform, grant = %id, "the grant names no remote user; not revoked at the provider");
            return;
        };
        let now = self.deps.clock.now();
        match OAuthStateRepo::remote_user_in_use(
            &mut tx,
            platform,
            remote,
            RemoteUserExcept::Grant(id),
            now,
        )
        .await
        {
            Ok(false) => {}
            Ok(true) => {
                tracing::info!(%platform, grant = %id, "the remote user is still in use; not revoked at the provider");
                return;
            }
            Err(error) => {
                tracing::warn!(%platform, grant = %id, ?error, "could not apply the revoke-skip rule; not revoked");
                return;
            }
        }
        let Some(tokens) = self.open_oauth_grant(&grant) else {
            tracing::info!(%platform, grant = %id, "the grant holds no usable credential; nothing revoked");
            return;
        };
        let config = match self.deps.catalog.resolve(&mut tx, platform).await {
            Ok(resolved) => resolved.config,
            Err(error) => {
                tracing::info!(%platform, %error, "the platform is unavailable; skipping the revoke");
                return;
            }
        };
        self.revoke_tokens(platform, &config, &tokens).await;
        drop(tx);
    }

    /// The credentials arm of [`AccountService::revoke_grant`]: the agent's `revoke()` with a
    /// client bound to the grant.
    async fn revoke_with_agent(&self, owner: UserId, grant: GrantId, platform: &PlatformId) {
        match self.resolve(platform).await {
            Ok(resolved) => {
                let ctx = self.agent_context(owner, grant, resolved.config.clone());
                match resolved.agent.create_client(ctx) {
                    Ok(client) => {
                        if let Err(error) = client.revoke().await {
                            tracing::debug!(%platform, error = agent_error_name(&error), "revoking the grant did not succeed");
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%platform, error = agent_error_name(&error), "no client to revoke the grant with");
                    }
                }
            }
            Err(error) => {
                tracing::debug!(%platform, ?error, "platform unavailable; skipping the revoke");
            }
        }
    }

    /// Best effort: revokes the tokens of a pending result already taken out of the table,
    /// under the revoke-skip rule with `except` left out of the check. Returns whether the
    /// tokens were sent to the provider. A result is dropped without a revoke when it names no
    /// remote user, when the remote user is in use, when its tokens do not open, when its
    /// platform no longer resolves, or when the database fails the check: the row is already
    /// gone, so a retry could not find it again.
    pub(crate) async fn revoke_result(
        &self,
        row: &OAuthState,
        except: RemoteUserExcept,
        now: DateTime<Utc>,
    ) -> bool {
        let platform = &row.platform;
        let Some(remote) = row.remote_user_id.as_deref() else {
            return false;
        };
        let in_use = match self.deps.pool.acquire().await {
            Ok(mut conn) => {
                OAuthStateRepo::remote_user_in_use(&mut conn, platform, remote, except, now).await
            }
            Err(error) => Err(error.into()),
        };
        match in_use {
            Ok(false) => {}
            Ok(true) => return false,
            Err(error) => {
                tracing::warn!(%platform, ?error, "could not apply the revoke-skip rule; a result is dropped without a revoke");
                return false;
            }
        }
        let Some(tokens) = self.open_result_tokens(row) else {
            tracing::warn!(%platform, "a result's tokens did not open; dropped");
            return false;
        };
        let config = match self.resolve(platform).await {
            Ok(resolved) => resolved.config,
            Err(error) => {
                tracing::info!(%platform, %error, "the platform is unavailable; a result is dropped without a revoke");
                return false;
            }
        };
        self.revoke_tokens(platform, &config, &tokens).await;
        true
    }

    /// The grant tokens sealed in a pending result.
    fn open_result_tokens(&self, row: &OAuthState) -> Option<OAuthTokens> {
        let sealed = row.grant_tokens.as_ref()?;
        let opened: Value = self
            .deps
            .vault
            .open(sealed, &OAuthStateRepo::seal_context(row.id, TOKENS_COLUMN))
            .ok()?;
        let stored: StoredExchange = serde_json::from_value(opened).ok()?;
        Some(OAuthTokens {
            access_token: SecretString::from(stored.access_token),
            refresh_token: stored.refresh_token.map(SecretString::from),
            expires_at: stored.expires_at,
            scopes: stored.scopes,
        })
    }
}

/// The user deletion step that ends a deleted user's OAuth authorizations at the providers,
/// before the user's row deletion cascades their grants and pending results away.
///
/// It first takes the user's pending results (live or discarded) and revokes each under the
/// revoke-skip rule; the user's own active grants count there, so a result sharing its remote
/// user with one of them is spared and the grant's revoke ends that authorization once. Then it
/// revokes each grant that is not yet `revoked` through [`AccountService::revoke_grant`] and
/// marks it `revoked` with its credential cleared, so a retry does not repeat the call.
pub struct OAuthDeletionStep {
    service: AccountService,
}

impl OAuthDeletionStep {
    /// The step over `service`.
    #[must_use]
    pub fn new(service: AccountService) -> Self {
        Self { service }
    }
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

#[async_trait]
impl UserDeletionStep for OAuthDeletionStep {
    fn name(&self) -> &'static str {
        "oauth"
    }

    async fn run(&self, user: UserId) -> Result<(), JobError> {
        let service = &self.service;
        let pool = &service.deps.pool;
        let now = service.deps.clock.now();

        let mut conn = pool.acquire().await.map_err(retry)?;
        let results = OAuthStateRepo::take_results_of(&mut conn, user)
            .await
            .map_err(retry)?;
        drop(conn);
        // The user's results are already gone, so nothing needs leaving out of the check.
        for row in &results {
            service
                .revoke_result(row, RemoteUserExcept::Nothing, now)
                .await;
        }

        let mut conn = pool.acquire().await.map_err(retry)?;
        let grants = GrantRepo::unrevoked_oauth_grants_of(&mut conn, user)
            .await
            .map_err(retry)?;
        drop(conn);
        for grant in &grants {
            service.revoke_grant(user, grant).await;
            let mut tx = pool.begin().await.map_err(retry)?;
            GrantRepo::set_revoked(&mut tx, grant.id)
                .await
                .map_err(retry)?;
            tx.commit().await.map_err(retry)?;
        }
        tracing::info!(
            results = results.len(),
            grants = grants.len(),
            "oauth authorizations of a deleted user ended"
        );
        Ok(())
    }
}
