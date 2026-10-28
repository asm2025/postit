//! Refreshing an OAuth grant's tokens: one function, under a per-grant lock, for the cron and
//! for the credential store, so the two can never disagree about a grant.

use chrono::{DateTime, Duration, Utc};
use postit_core::OAuthProviderSpec;
use postit_core::{
    AgentError, AuditEventId, ConnectMethod, GrantId, OAuthTokens, PlatformId, UserId,
};
use postit_data::DataError;
use postit_data::accounts::{Grant, GrantKind, GrantRepo, GrantStatus};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use secrecy::{ExposeSecret as _, SecretString};
use sha2::Digest as _;
use sqlx::PgConnection;

use crate::connect::OAuthGrantCredential;
use crate::engine::{self, ClientCredentials, TokenError};
use crate::{AccountError, AccountService};

/// How long before its access token expires a grant is refreshed by the cron.
pub const REFRESH_WINDOW: Duration = Duration::minutes(15);

pub(crate) fn reconnect() -> AgentError {
    AgentError::Authentication {
        message: "the credential is not usable; reconnect the account".to_owned(),
    }
}

pub(crate) fn db_unavailable() -> AgentError {
    AgentError::Transport {
        message: "database unavailable".to_owned(),
    }
}

/// What tells two sets of tokens apart without holding or printing either: the expiry and a
/// SHA-256 of the access token.
fn fingerprint(tokens: &OAuthTokens) -> (Option<DateTime<Utc>>, [u8; 32]) {
    let digest = sha2::Sha256::digest(tokens.access_token.expose_secret().as_bytes());
    (tokens.expires_at, digest.into())
}

/// The OAuth error code of a token failure, for an error that names no secret.
fn token_error_code(error: &TokenError) -> String {
    match error {
        TokenError::InvalidGrant => "invalid_grant".to_owned(),
        TokenError::InvalidClient => "invalid_client".to_owned(),
        TokenError::Provider(code) => code.clone(),
        TokenError::Transport => "transport".to_owned(),
    }
}

impl AccountService {
    /// Gets new tokens for an OAuth grant, under the grant's lock, so concurrent callers in any
    /// process make one token request between them.
    ///
    /// After taking the lock it re-reads the grant: tokens that differ from `stale` (another
    /// caller refreshed) are returned as they are. With `stale = None` (the cron) the grant is
    /// refreshed when it is still within [`REFRESH_WINDOW`] of expiring. A grant with no
    /// refresh token never reaches the provider: its tokens are returned while they are
    /// unexpired, and once expired the grant and its accounts go to `needs_reconnect`.
    /// `invalid_grant` does the same; a retryable failure leaves the grant as it was.
    ///
    /// # Errors
    ///
    /// [`AgentError::Authentication`] when the grant is missing, not an active OAuth grant, or
    /// must be reconnected; [`AgentError::Transport`] when the database, the platform, or the
    /// provider is unavailable for now; [`AgentError::Remote`] for another provider refusal.
    pub async fn refresh_grant(
        &self,
        grant: GrantId,
        stale: Option<&OAuthTokens>,
    ) -> Result<OAuthTokens, AgentError> {
        self.refresh_grant_of(grant, None, stale).await
    }

    /// [`AccountService::refresh_grant`], refused unless the grant belongs to `owner` when
    /// given.
    pub(crate) async fn refresh_grant_of(
        &self,
        id: GrantId,
        owner: Option<UserId>,
        stale: Option<&OAuthTokens>,
    ) -> Result<OAuthTokens, AgentError> {
        // The transaction holds the grant's lock across the token request by design: that is
        // what keeps a second caller from sending the same refresh token.
        let mut tx = self.deps.pool.begin().await.map_err(|_| db_unavailable())?;
        let Some((grant_owner, grant)) = GrantRepo::lock_for_refresh(&mut tx, id)
            .await
            .map_err(|_| db_unavailable())?
        else {
            return Err(reconnect());
        };
        if owner.is_some_and(|owner| owner != grant_owner)
            || grant.kind != GrantKind::Oauth
            || grant.status != GrantStatus::Active
        {
            return Err(reconnect());
        }
        let current = self.open_oauth_grant(&grant).ok_or_else(reconnect)?;
        let now = self.deps.clock.now();
        match stale {
            Some(stale) if fingerprint(stale) != fingerprint(&current) => return Ok(current),
            None if current
                .expires_at
                .is_none_or(|at| at >= now + REFRESH_WINDOW) =>
            {
                return Ok(current);
            }
            _ => {}
        }

        let Some(refresh_token) = current.refresh_token.clone() else {
            if current.expires_at.is_none_or(|at| at > now) {
                return Ok(current);
            }
            self.mark_reconnect(&mut tx, id, &grant.platform)
                .await
                .map_err(|_| db_unavailable())?;
            tx.commit().await.map_err(|_| db_unavailable())?;
            return Err(reconnect());
        };

        let (spec, client) = self.refresh_client(&mut tx, &grant.platform).await?;
        match engine::refresh(&self.deps.http, &spec, &client, &refresh_token, now).await {
            Ok(exchange) => {
                let fresh = OAuthTokens {
                    access_token: exchange.tokens.access_token,
                    // A provider that does not rotate sends none: the old one stays valid.
                    refresh_token: exchange.tokens.refresh_token.or(Some(refresh_token)),
                    expires_at: exchange.tokens.expires_at,
                    scopes: grant.scopes.clone(),
                };
                self.write_grant_tokens(&mut tx, id, &fresh)
                    .await
                    .map_err(|_| db_unavailable())?;
                tx.commit().await.map_err(|_| db_unavailable())?;
                Ok(fresh)
            }
            Err(TokenError::InvalidGrant) => {
                tracing::info!(grant = %id, "the provider refused the refresh token; the grant needs a reconnect");
                self.mark_reconnect(&mut tx, id, &grant.platform)
                    .await
                    .map_err(|_| db_unavailable())?;
                tx.commit().await.map_err(|_| db_unavailable())?;
                Err(reconnect())
            }
            Err(error) if error.retryable() => {
                // Dropping the transaction rolls it back and releases the lock.
                drop(tx);
                Err(AgentError::Transport {
                    message: "the token refresh failed; it is tried again later".to_owned(),
                })
            }
            Err(error) => {
                drop(tx);
                Err(AgentError::Remote {
                    status: None,
                    code: Some(token_error_code(&error)),
                    message: "the provider refused the token refresh".to_owned(),
                })
            }
        }
    }

    /// An OAuth grant's tokens: the sealed credential opened, with the expiry and scopes of
    /// its row. `None` when it has no credential or the credential does not open.
    pub(crate) fn open_oauth_grant(&self, grant: &Grant) -> Option<OAuthTokens> {
        let sealed = grant.credential.as_ref()?;
        let opened: serde_json::Value = self
            .deps
            .vault
            .open(sealed, &GrantRepo::seal_context(grant.id))
            .ok()?;
        let opened: OAuthGrantCredential = serde_json::from_value(opened).ok()?;
        Some(OAuthTokens {
            access_token: SecretString::from(opened.access_token),
            refresh_token: opened.refresh_token.map(SecretString::from),
            expires_at: grant.expires_at,
            scopes: grant.scopes.clone(),
        })
    }

    /// Seals `tokens` into the grant's credential and stores their expiry. The caller holds
    /// the grant's lock.
    pub(crate) async fn write_grant_tokens(
        &self,
        tx: &mut PgConnection,
        id: GrantId,
        tokens: &OAuthTokens,
    ) -> Result<(), AccountError> {
        let credential = serde_json::to_value(OAuthGrantCredential {
            access_token: tokens.access_token.expose_secret().to_owned(),
            refresh_token: tokens
                .refresh_token
                .as_ref()
                .map(|token| token.expose_secret().to_owned()),
        })
        .map_err(|_| AccountError::Data(DataError::Conflict("grant tokens".to_owned())))?;
        let sealed = self
            .deps
            .vault
            .seal(&credential, &GrantRepo::seal_context(id))?;
        GrantRepo::store_tokens(tx, id, &sealed, tokens.expires_at).await?;
        Ok(())
    }

    /// Sets the grant and its accounts that are not disconnected to `needs_reconnect`, and
    /// records `account_needs_reconnect` for each account, with no actor.
    pub(crate) async fn mark_reconnect(
        &self,
        tx: &mut PgConnection,
        id: GrantId,
        platform: &PlatformId,
    ) -> Result<(), AccountError> {
        for (owner, account) in GrantRepo::mark_needs_reconnect(tx, id).await? {
            let event = AuditEvent::new(AuditEventKind::AccountNeedsReconnect)
                .owner(owner)
                .detail("account_id", account.to_string())
                .and_then(|e| e.detail("platform", platform.as_str()))
                .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
            AuditLog::record(tx, AuditEventId::from(self.deps.ids.generate()), event).await?;
        }
        Ok(())
    }

    /// The platform's OAuth spec and the deployment's client, read on `conn`.
    async fn refresh_client(
        &self,
        conn: &mut PgConnection,
        platform: &PlatformId,
    ) -> Result<(OAuthProviderSpec, ClientCredentials), AgentError> {
        let unavailable = || AgentError::Transport {
            message: "the platform is unavailable".to_owned(),
        };
        let resolved = self
            .deps
            .catalog
            .resolve(conn, platform)
            .await
            .map_err(|error| {
                tracing::warn!(%platform, %error, "cannot refresh: the platform is unavailable");
                unavailable()
            })?;
        let ConnectMethod::OAuth(spec) = &resolved.agent.descriptor().connect else {
            return Err(reconnect());
        };
        let client = ClientCredentials::from_config(&resolved.config).map_err(|_| unavailable())?;
        Ok((spec.clone(), client))
    }
}
