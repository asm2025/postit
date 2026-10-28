//! The credential store an agent client reads its grant's credentials through.

use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use postit_core::{
    AccountRef, AccountToken, AgentError, Capability, CredentialFields, CredentialStore,
    Credentials, GrantId, OAuthTokens, UserId,
};
use postit_data::accounts::{AccountRepo, AccountTokenRepo, GrantKind, GrantRepo, GrantStatus};
use postit_data::{DataError, OwnerScope};
use secrecy::SecretString;
use serde_json::{Map, Value};

use crate::AccountService;
use crate::refresh::{db_unavailable, reconnect};

/// Reads one grant's credentials from the database and opens them with the vault. An OAuth
/// grant's tokens are refreshed through [`AccountService::refresh_grant`].
///
/// A grant that is missing, not `active`, of another owner, or without a credential answers
/// [`AgentError::Authentication`]: the user must reconnect.
pub struct PgCredentialStore {
    service: AccountService,
    owner: UserId,
    grant: GrantId,
}

impl PgCredentialStore {
    /// A store for `grant`, which belongs to `owner`, over the service's database, vault, and
    /// OAuth engine.
    #[must_use]
    pub fn new(service: AccountService, owner: UserId, grant: GrantId) -> Self {
        Self {
            service,
            owner,
            grant,
        }
    }
}

impl fmt::Debug for PgCredentialStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgCredentialStore")
            .field("grant", &self.grant)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CredentialStore for PgCredentialStore {
    async fn credentials(&self) -> Result<Credentials, AgentError> {
        let deps = &self.service.deps;
        let mut conn = deps.pool.acquire().await.map_err(|_| db_unavailable())?;
        let grant = match GrantRepo::get(&mut conn, &OwnerScope::own(self.owner), self.grant).await
        {
            Ok(grant) => grant,
            Err(DataError::NotFound) => return Err(reconnect()),
            // A database failure says nothing about the grant: retry, never "reconnect".
            Err(_) => return Err(db_unavailable()),
        };
        // The read connection goes back before any refresh, which holds its own transaction
        // across the token request.
        drop(conn);
        if grant.status != GrantStatus::Active {
            return Err(reconnect());
        }
        match grant.kind {
            GrantKind::Oauth => {
                let tokens = self
                    .service
                    .open_oauth_grant(&grant)
                    .ok_or_else(reconnect)?;
                let expired = tokens.expires_at.is_some_and(|at| at <= deps.clock.now());
                if !expired {
                    return Ok(Credentials::OAuth(tokens));
                }
                self.service
                    .refresh_grant_of(self.grant, Some(self.owner), Some(&tokens))
                    .await
                    .map(Credentials::OAuth)
            }
            GrantKind::Credentials => {
                // A revoked grant keeps no credential: the user must reconnect.
                let Some(sealed) = &grant.credential else {
                    return Err(reconnect());
                };
                let secrets: Value = deps
                    .vault
                    .open(sealed, &GrantRepo::seal_context(self.grant))
                    .map_err(|_| reconnect())?;
                let (Value::Object(plain), Value::Object(secrets)) =
                    (grant.connection_config, secrets)
                else {
                    return Err(reconnect());
                };
                Ok(Credentials::Fields(CredentialFields::from_parts(
                    plain,
                    into_secrets(secrets),
                )))
            }
        }
    }

    async fn store_refreshed(&self, tokens: OAuthTokens) -> Result<(), AgentError> {
        let deps = &self.service.deps;
        let mut tx = deps.pool.begin().await.map_err(|_| db_unavailable())?;
        let Some((owner, grant)) = GrantRepo::lock_for_refresh(&mut tx, self.grant)
            .await
            .map_err(|_| db_unavailable())?
        else {
            return Err(reconnect());
        };
        if owner != self.owner {
            return Err(reconnect());
        }
        if grant.kind != GrantKind::Oauth {
            // Pasted credentials never refresh.
            return Err(AgentError::Unsupported {
                capability: Capability::Commenter,
            });
        }
        if grant.status != GrantStatus::Active {
            return Err(reconnect());
        }
        let current = self
            .service
            .open_oauth_grant(&grant)
            .ok_or_else(reconnect)?;
        let tokens = OAuthTokens {
            refresh_token: tokens.refresh_token.or(current.refresh_token),
            ..tokens
        };
        self.service
            .write_grant_tokens(&mut tx, self.grant, &tokens)
            .await
            .map_err(|_| db_unavailable())?;
        tx.commit().await.map_err(|_| db_unavailable())?;
        Ok(())
    }

    async fn refresh(&self, stale: &OAuthTokens) -> Result<OAuthTokens, AgentError> {
        self.service
            .refresh_grant_of(self.grant, Some(self.owner), Some(stale))
            .await
    }

    async fn account_token(
        &self,
        account: &AccountRef,
    ) -> Result<Option<AccountToken>, AgentError> {
        let deps = &self.service.deps;
        let scope = OwnerScope::own(self.owner);
        let mut conn = deps.pool.acquire().await.map_err(|_| db_unavailable())?;
        // Only an account of this grant's owner, on this grant, under the remote ID asked for.
        let row = match AccountRepo::get(&mut conn, &scope, account.account).await {
            Ok(row) => row,
            Err(DataError::NotFound) => return Ok(None),
            // A database failure says nothing about the token: retry, never "no token".
            Err(_) => return Err(db_unavailable()),
        };
        if row.grant_id != Some(self.grant) || row.remote_account_id != account.remote_id {
            return Ok(None);
        }
        let Some((sealed, expires_at)) = AccountTokenRepo::get(&mut conn, &scope, row.id)
            .await
            .map_err(|_| db_unavailable())?
        else {
            return Ok(None);
        };
        drop(conn);
        let opened: Value = deps
            .vault
            .open(&sealed, &AccountTokenRepo::seal_context(row.id))
            .map_err(|_| reconnect())?;
        let Value::String(token) = opened else {
            return Err(reconnect());
        };
        Ok(Some(AccountToken {
            access_token: SecretString::from(token),
            expires_at,
        }))
    }
}

fn into_secrets(values: Map<String, Value>) -> BTreeMap<String, SecretString> {
    values
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::String(text) => Some((name, SecretString::from(text))),
            _ => None,
        })
        .collect()
}
