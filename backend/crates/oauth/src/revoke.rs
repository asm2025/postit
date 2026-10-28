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
use serde_json::Value;
use sqlx::PgConnection;

use crate::engine::{self, ClientCredentials};
use crate::error::agent_error_name;
use crate::flow::{ExchangedCredentials, StoredExchange, TOKENS_COLUMN};
use crate::{AccountError, AccountService};

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

    /// The OAuth arm of [`AccountService::revoke_grant`]:
    /// [`AccountService::try_revoke_oauth_grant`] with a database failure logged and swallowed.
    async fn revoke_oauth_grant(&self, id: GrantId) {
        if let Err(error) = self.try_revoke_oauth_grant(id).await {
            tracing::warn!(grant = %id, %error, "the database failed; the grant was not revoked at the provider");
        }
    }

    /// Revokes an OAuth grant at the provider under the revoke-skip rule, holding the grant's
    /// refresh lock across the provider call. No call is made, and `Ok` returned, when the
    /// grant is gone, names no remote user, has its remote user in use, holds no usable
    /// credential, or its platform no longer resolves (unknown, disabled, or unconfigured). A
    /// failed provider call is logged and also returns `Ok`: revoking is best effort.
    ///
    /// # Errors
    ///
    /// [`AccountError::Data`] when the database fails before the call, so a caller that must
    /// not lose the revoke (the deletion step) can retry it.
    pub(crate) async fn try_revoke_oauth_grant(&self, id: GrantId) -> Result<(), AccountError> {
        // The transaction holds the grant's refresh lock across the provider call by design.
        // Nothing is written: dropping it at the end rolls back and releases the lock.
        let mut tx = self.deps.pool.begin().await?;
        let Some((_, grant)) = GrantRepo::lock_for_refresh(&mut tx, id).await? else {
            return Ok(());
        };
        let platform = &grant.platform;
        let Some(remote) = grant.remote_user_id.as_deref() else {
            tracing::info!(%platform, grant = %id, "the grant names no remote user; not revoked at the provider");
            return Ok(());
        };
        let now = self.deps.clock.now();
        if OAuthStateRepo::remote_user_in_use(
            &mut tx,
            platform,
            remote,
            RemoteUserExcept::Grant(id),
            now,
        )
        .await?
        {
            tracing::info!(%platform, grant = %id, "the remote user is still in use; not revoked at the provider");
            return Ok(());
        }
        let Some(tokens) = self.open_oauth_grant(&grant) else {
            tracing::info!(%platform, grant = %id, "the grant holds no usable credential; nothing revoked");
            return Ok(());
        };
        let Some(config) = self.resolve_for_revoke(&mut tx, platform).await? else {
            return Ok(());
        };
        self.revoke_tokens(platform, &config, &tokens).await;
        drop(tx);
        Ok(())
    }

    /// The platform's config for a revoke, read on `conn`. `None` when the platform no longer
    /// resolves (unknown, disabled, or unconfigured): a reason to drop the revoke. A database
    /// failure is an error, never a reason to drop.
    async fn resolve_for_revoke(
        &self,
        conn: &mut PgConnection,
        platform: &PlatformId,
    ) -> Result<Option<Value>, AccountError> {
        match self.deps.catalog.resolve(conn, platform).await {
            Ok(resolved) => Ok(Some(resolved.config)),
            Err(
                error @ (AccountError::PlatformNotFound
                | AccountError::PlatformDisabled
                | AccountError::PlatformUnavailable),
            ) => {
                tracing::info!(%platform, %error, "the platform is unavailable; dropped without a revoke");
                Ok(None)
            }
            Err(other) => Err(other),
        }
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

    /// Decides, on `conn` (the transaction that took `row`), whether a taken pending result
    /// is revoked: the revoke-skip rule with `except` left out of the check, then the
    /// platform's config. Returns the tokens and config to revoke with, or `None` to drop the
    /// result without a revoke: it names no remote user, the remote user is in use, its tokens
    /// do not open, or its platform no longer resolves. The caller commits the take only after
    /// this succeeds and calls the provider after the commit, so a database failure rolls the
    /// take back and the result is retried, never dropped.
    ///
    /// # Errors
    ///
    /// [`AccountError::Data`] when the database fails.
    pub(crate) async fn plan_result_revoke(
        &self,
        conn: &mut PgConnection,
        row: &OAuthState,
        except: RemoteUserExcept,
        now: DateTime<Utc>,
    ) -> Result<Option<(OAuthTokens, Value)>, AccountError> {
        let platform = &row.platform;
        let Some(remote) = row.remote_user_id.as_deref() else {
            return Ok(None);
        };
        if OAuthStateRepo::remote_user_in_use(conn, platform, remote, except, now).await? {
            return Ok(None);
        }
        let Some(tokens) = self.open_result_tokens(row) else {
            tracing::warn!(%platform, "a result's tokens did not open; dropped");
            return Ok(None);
        };
        Ok(self
            .resolve_for_revoke(conn, platform)
            .await?
            .map(|config| (tokens, config)))
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
            access_token: stored.access_token,
            refresh_token: stored.refresh_token,
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
/// revokes each grant that is not yet `revoked` as [`AccountService::revoke_grant`] does and
/// marks it `revoked` with its credential cleared, so a retry does not repeat the call. A
/// database failure before a provider call retries the step and loses no revoke.
///
/// A `needs_reconnect` grant does not hold its remote user in use, so a result of the same
/// remote user is revoked with its own tokens and the grant with its own: two calls. They hold
/// different token sets, both of which a provider that revokes per token must see; on a
/// provider that revokes per user and app pair the second RFC 7009 call is a harmless no-op.
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

        // Take the user's results, plan each revoke, and commit only when every plan
        // succeeded; the provider calls come after the commit, on no connection.
        let mut tx = pool.begin().await.map_err(retry)?;
        let results = OAuthStateRepo::take_results_of(&mut tx, user)
            .await
            .map_err(retry)?;
        let mut planned = Vec::new();
        for row in &results {
            // The user's results are already taken, so nothing needs leaving out of the check.
            if let Some((tokens, config)) = service
                .plan_result_revoke(&mut tx, row, RemoteUserExcept::Nothing, now)
                .await
                .map_err(retry)?
            {
                planned.push((&row.platform, tokens, config));
            }
        }
        tx.commit().await.map_err(retry)?;
        for (platform, tokens, config) in &planned {
            service.revoke_tokens(platform, config, tokens).await;
        }

        let mut conn = pool.acquire().await.map_err(retry)?;
        let grants = GrantRepo::unrevoked_oauth_grants_of(&mut conn, user)
            .await
            .map_err(retry)?;
        drop(conn);
        for grant in &grants {
            // A database failure retries the step before the grant is marked, so its
            // credential is still there for the retry to revoke.
            service
                .try_revoke_oauth_grant(grant.id)
                .await
                .map_err(retry)?;
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
