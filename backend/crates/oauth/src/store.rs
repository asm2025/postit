//! The credential store an agent client reads its grant's credentials through.

use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use postit_core::{
    AgentError, Capability, CredentialFields, CredentialStore, Credentials, GrantId, OAuthTokens,
    UserId,
};
use postit_data::OwnerScope;
use postit_data::accounts::{GrantKind, GrantRepo, GrantStatus};
use postit_vault::Vault;
use secrecy::SecretString;
use serde_json::{Map, Value};
use sqlx::PgPool;

/// Reads one grant's credentials from the database and opens them with the vault.
///
/// A grant that is missing, not `active`, or not a credentials grant answers
/// [`AgentError::Authentication`]: the user must reconnect.
pub struct PgCredentialStore {
    pool: PgPool,
    vault: Vault,
    owner: UserId,
    grant: GrantId,
}

impl PgCredentialStore {
    /// A store for `grant`, which belongs to `owner`.
    #[must_use]
    pub fn new(pool: PgPool, vault: Vault, owner: UserId, grant: GrantId) -> Self {
        Self {
            pool,
            vault,
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

fn reconnect() -> AgentError {
    AgentError::Authentication {
        message: "the credential is not usable; reconnect the account".to_owned(),
    }
}

#[async_trait]
impl CredentialStore for PgCredentialStore {
    async fn credentials(&self) -> Result<Credentials, AgentError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|_| AgentError::Transport {
                message: "database unavailable".to_owned(),
            })?;
        let scope = OwnerScope::own(self.owner);
        let grant = GrantRepo::get(&mut conn, &scope, self.grant)
            .await
            .map_err(|_| reconnect())?;
        if grant.status != GrantStatus::Active || grant.kind != GrantKind::Credentials {
            return Err(reconnect());
        }
        // A revoked grant keeps no credential: the user must reconnect.
        let Some(sealed) = &grant.credential else {
            return Err(reconnect());
        };
        let secrets: Value = self
            .vault
            .open(sealed, &GrantRepo::seal_context(self.grant))
            .map_err(|_| reconnect())?;
        let (Value::Object(plain), Value::Object(secrets)) = (grant.connection_config, secrets)
        else {
            return Err(reconnect());
        };
        Ok(Credentials::Fields(CredentialFields::from_parts(
            plain,
            into_secrets(secrets),
        )))
    }

    async fn store_refreshed(&self, _tokens: OAuthTokens) -> Result<(), AgentError> {
        // Pasted credentials never refresh; B7 adds the OAuth arm.
        Err(AgentError::Unsupported {
            capability: Capability::Commenter,
        })
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
