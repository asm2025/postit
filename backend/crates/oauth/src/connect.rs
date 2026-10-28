//! Connecting accounts with pasted credentials: check the credential, then confirm the
//! accounts it reaches. Confirming an OAuth result left by the callback runs here too.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use emixcrypto::HashAlgorithm as _;
use postit_core::{
    AccountId, AccountToken, AuditEventId, ConnectMethod, CredentialFields, DiscoveredAccount,
    GrantId, PendingConnectionId, PlatformId, Violation,
};
use postit_data::accounts::{
    Account, AccountRepo, AccountTokenRepo, AccountUpsert, GrantKind, GrantRepo, NewAccount,
    NewGrant,
};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::oauth_states::{OAuthState, OAuthStateRepo};
use postit_data::pending_connections::{
    NewPendingConnection, PendingConnection, PendingConnectionRepo,
};
use postit_data::{Capability, DataError, OwnerScope};
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Postgres, Transaction};

use crate::flow::{StoredExchange, TOKENS_COLUMN, stored_accounts};
use crate::manage::ConnectKind;
use crate::{AccountError, AccountService};

/// How long a checked credential waits for the user to confirm.
const PENDING_TTL: Duration = Duration::minutes(10);

/// The secret that lets the user who checked a credential confirm which accounts to connect.
/// Returned once; only its SHA-256 is stored.
pub struct ConnectId(String);

impl ConnectId {
    pub(crate) fn generate() -> Self {
        Self(random_secret())
    }

    /// The value to hand to the user.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The SHA-256 stored in `pending_connections.connect_id_hash`.
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        hash_of(&self.0)
    }

    /// The hash a presented `connect_id` string is looked up by.
    #[must_use]
    pub fn hash_of(presented: &str) -> [u8; 32] {
        hash_of(presented)
    }
}

impl fmt::Debug for ConnectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConnectId([redacted])")
    }
}

/// 32 random bytes, base64url without padding (43 characters): a `connect_id` or an OAuth
/// `state`.
pub(crate) fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The SHA-256 a secret is stored and looked up by.
pub(crate) fn hash_of(text: &str) -> [u8; 32] {
    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(text.as_bytes())
        .unwrap_or_default();
    let mut out = [0u8; 32];
    for (slot, byte) in out.iter_mut().zip(digest.iter()) {
        *slot = *byte;
    }
    out
}

/// The result of checking a pasted credential.
#[derive(Debug)]
pub struct CredentialCheck {
    /// The ID to confirm with and when it expires; `None` when the credential reaches no
    /// account, so there is nothing to confirm.
    pub pending: Option<(ConnectId, DateTime<Utc>)>,
    /// The accounts the credential reaches.
    pub discovered: Vec<DiscoveredAccount>,
}

/// What confirming did to one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountChange {
    /// A new account.
    Created,
    /// The owner's disconnected account, or one on the same grant, is active again.
    Reactivated,
    /// The owner's account moved from an older credential to this one.
    Moved,
}

impl AccountChange {
    /// The stable name used in audit details and responses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Reactivated => "reactivated",
            Self::Moved => "moved",
        }
    }
}

/// The result of confirming a connection.
#[derive(Debug)]
pub struct Confirmed {
    /// How the accounts were connected.
    pub connect: ConnectKind,
    /// The accounts now connected.
    pub connected: Vec<(Account, AccountChange)>,
    /// How many chosen accounts another owner already has. A count only: nothing says who.
    pub skipped_elsewhere: u32,
    /// Chosen IDs the credential did not reach.
    pub unknown: Vec<String>,
}

/// A checked credential, ready to store.
pub(crate) struct Verified {
    pub fields: CredentialFields,
    pub discovered: Vec<DiscoveredAccount>,
}

/// The sealed credential of an OAuth grant: its tokens. The grant's expiry and scopes are
/// columns of its row.
#[derive(Serialize, Deserialize)]
pub(crate) struct OAuthGrantCredential {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// The accounts to connect and the credential to connect them with.
pub(crate) struct ConnectInput<'a> {
    pub platform: &'a PlatformId,
    /// How the credential was obtained.
    pub kind: GrantKind,
    /// The credential's secret part, sealed into the grant.
    pub secrets: Value,
    /// The credential's plain part.
    pub config: Value,
    /// The provider's ID for the user who authorized an OAuth grant.
    pub remote_user_id: Option<String>,
    /// When the grant's access token expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// When the user must reconnect regardless of refreshing.
    pub reconnect_by: Option<DateTime<Utc>>,
    /// The scopes the grant carries; its accounts carry them too.
    pub scopes: Vec<String>,
    /// Per-account tokens, keyed by remote account ID.
    pub account_tokens: BTreeMap<String, AccountToken>,
    pub accounts: Vec<&'a DiscoveredAccount>,
    /// Replaces the display name; used by the CSV import for single-account rows.
    pub display_name: Option<&'a str>,
}

impl<'a> ConnectInput<'a> {
    /// Pasted credentials: no expiry, scopes, remote user, or account tokens.
    pub(crate) fn credentials(
        platform: &'a PlatformId,
        secrets: Value,
        config: Value,
        accounts: Vec<&'a DiscoveredAccount>,
        display_name: Option<&'a str>,
    ) -> Self {
        Self {
            platform,
            kind: GrantKind::Credentials,
            secrets,
            config,
            remote_user_id: None,
            expires_at: None,
            reconnect_by: None,
            scopes: Vec::new(),
            account_tokens: BTreeMap::new(),
            accounts,
            display_name,
        }
    }
}

/// Keeps what identifies a violation (the rule and the field) and drops free-form values.
pub(crate) fn redact_violations(violations: Vec<Violation>) -> Vec<Violation> {
    violations
        .into_iter()
        .map(|mut v| {
            v.params
                .retain(|key, value| !value.is_string() || key == "name");
            v.fix = None;
            v
        })
        .collect()
}

impl AccountService {
    /// Checks pasted credentials against the platform and lists the accounts they reach.
    /// When there are any, stores the credential sealed and returns the ID to confirm with.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, a platform error from the
    /// catalog, [`AccountError::ConnectMethodUnsupported`] for an OAuth platform,
    /// [`AccountError::Validation`] for values that do not fit the schema, and
    /// [`AccountError::CredentialsInvalid`] or another error when the platform refuses them.
    pub async fn start_credentials(
        &self,
        scope: &OwnerScope,
        platform: &PlatformId,
        values: Map<String, Value>,
    ) -> Result<CredentialCheck, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let verified = self.verify(platform, values).await?;
        if verified.discovered.is_empty() {
            return Ok(CredentialCheck {
                pending: None,
                discovered: Vec::new(),
            });
        }

        let (plain, secret) = verified.fields.split();
        let id = PendingConnectionId::from(self.deps.ids.generate());
        let credential = self
            .deps
            .vault
            .seal(&json!(secret), &PendingConnectionRepo::seal_context(id))?;
        let connect_id = ConnectId::generate();
        let expires_at = self.deps.clock.now() + PENDING_TTL;
        let discovered = serde_json::to_value(&verified.discovered).map_err(|_| {
            AccountError::CsvInvalid("discovered accounts could not be stored".to_owned())
        })?;
        let mut conn = self.deps.pool.acquire().await?;
        PendingConnectionRepo::insert(
            &mut conn,
            scope,
            NewPendingConnection {
                id,
                platform: platform.clone(),
                connect_id_hash: connect_id.hash(),
                credential,
                connection_config: Value::Object(plain),
                discovered,
                expires_at,
            },
        )
        .await?;
        Ok(CredentialCheck {
            pending: Some((connect_id, expires_at)),
            discovered: verified.discovered,
        })
    }

    /// Parses `values` against the platform's credentials schema and asks the agent to verify
    /// them. Holds no database connection during the platform call.
    pub(crate) async fn verify(
        &self,
        platform: &PlatformId,
        values: Map<String, Value>,
    ) -> Result<Verified, AccountError> {
        let resolved = self.resolve(platform).await?;
        let ConnectMethod::Credentials(schema) = &resolved.agent.descriptor().connect else {
            return Err(AccountError::ConnectMethodUnsupported);
        };
        let fields = schema
            .parse_credentials(values)
            .map_err(|violations| AccountError::Validation(redact_violations(violations)))?;
        let discovered = resolved
            .agent
            .verify_credentials(&self.check_context(resolved.config.clone()), &fields)
            .await
            .map_err(AccountError::from_agent)?;
        Ok(Verified {
            fields,
            discovered: discovered.accounts,
        })
    }

    /// Connects the accounts the user chose from a checked credential or an OAuth result.
    ///
    /// A pending connection is single use: whoever presents the `connect_id` ends it, and only
    /// the user who started it gets accounts. A wrong user, an expired connection, and a
    /// platform an admin has since switched off all end it and answer as shown: a credential
    /// connection is deleted, an OAuth result discarded so the purge revokes its tokens. An
    /// OAuth result that connects nothing (none chosen, or every chosen account connected to
    /// another owner) is discarded too; one that connects any account is deleted, its tokens
    /// now in the grant. A connect that fails (such as [`AccountError::SendingMethodExists`])
    /// rolls back, so the connection stays valid until it expires and the user can confirm
    /// again after fixing the cause. No provider is called.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, [`AccountError::NotFound`] for
    /// an unknown, expired, or foreign `connect_id`, the catalog's errors when the platform is
    /// no longer available, and [`AccountError::SendingMethodExists`].
    pub async fn confirm(
        &self,
        scope: &OwnerScope,
        connect_id: &str,
        account_ids: Option<Vec<String>>,
        audit: AuditContext,
    ) -> Result<Confirmed, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let hash = hash_of(connect_id);
        let mut tx = self.deps.pool.begin().await?;
        if let Some(pending) = PendingConnectionRepo::take(&mut tx, &hash).await? {
            return self
                .confirm_credentials(tx, scope, pending, account_ids, audit)
                .await;
        }
        let Some(result) = OAuthStateRepo::lock_result(&mut tx, &hash).await? else {
            return Err(AccountError::NotFound);
        };
        self.confirm_oauth(tx, scope, result, account_ids, audit)
            .await
    }

    /// Confirms a checked credential, already taken from `pending_connections` in `tx`.
    async fn confirm_credentials(
        &self,
        mut tx: Transaction<'static, Postgres>,
        scope: &OwnerScope,
        pending: PendingConnection,
        account_ids: Option<Vec<String>>,
        audit: AuditContext,
    ) -> Result<Confirmed, AccountError> {
        if pending.owner != scope.owner || pending.expires_at <= self.deps.clock.now() {
            tx.commit().await?;
            return Err(AccountError::NotFound);
        }
        if let Err(error) = self.deps.catalog.resolve(&mut tx, &pending.platform).await {
            tx.commit().await?;
            return Err(error);
        }

        let secrets: Value = self.deps.vault.open(
            &pending.credential,
            &PendingConnectionRepo::seal_context(pending.id),
        )?;
        let discovered = stored_accounts(pending.discovered)?;
        let (chosen, unknown) = choose(&discovered, account_ids);
        if chosen.is_empty() {
            tx.commit().await?;
            return Ok(Confirmed {
                connect: ConnectKind::Credentials,
                connected: Vec::new(),
                skipped_elsewhere: 0,
                unknown,
            });
        }

        let mut confirmed = self
            .connect_in_tx(
                &mut tx,
                scope,
                ConnectInput::credentials(
                    &pending.platform,
                    secrets,
                    pending.connection_config,
                    chosen,
                    None,
                ),
                audit,
            )
            .await?;
        confirmed.unknown = unknown;
        tx.commit().await?;
        Ok(confirmed)
    }

    /// Confirms an OAuth result, locked in `tx`: moves its tokens into a new grant, or
    /// discards it for the purge to revoke when nothing gets connected.
    async fn confirm_oauth(
        &self,
        mut tx: Transaction<'static, Postgres>,
        scope: &OwnerScope,
        result: OAuthState,
        account_ids: Option<Vec<String>>,
        audit: AuditContext,
    ) -> Result<Confirmed, AccountError> {
        let now = self.deps.clock.now();
        if result.user != scope.owner || result.expires_at <= now {
            OAuthStateRepo::discard(&mut tx, result.id, now).await?;
            tx.commit().await?;
            return Err(AccountError::NotFound);
        }
        if let Err(error) = self.deps.catalog.resolve(&mut tx, &result.platform).await {
            OAuthStateRepo::discard(&mut tx, result.id, now).await?;
            tx.commit().await?;
            return Err(error);
        }

        let (Some(sealed), Some(discovered)) = (&result.grant_tokens, result.discovered) else {
            return Err(AccountError::Data(DataError::Conflict(
                "stored result".to_owned(),
            )));
        };
        let opened: Value = self.deps.vault.open(
            sealed,
            &OAuthStateRepo::seal_context(result.id, TOKENS_COLUMN),
        )?;
        let exchanged: StoredExchange = serde_json::from_value(opened)
            .map_err(|_| AccountError::Data(DataError::Conflict("stored tokens".to_owned())))?;
        let discovered = stored_accounts(discovered)?;
        let (chosen, unknown) = choose(&discovered, account_ids);

        let mut confirmed = if chosen.is_empty() {
            Confirmed {
                connect: ConnectKind::OAuth,
                connected: Vec::new(),
                skipped_elsewhere: 0,
                unknown: Vec::new(),
            }
        } else {
            let input = oauth_input(&result.platform, exchanged, chosen)?;
            self.connect_in_tx(&mut tx, scope, input, audit).await?
        };
        confirmed.unknown = unknown;

        if confirmed.connected.is_empty() {
            // The grant `connect_in_tx` made is already gone; the purge revokes the tokens.
            OAuthStateRepo::discard(&mut tx, result.id, now).await?;
        } else {
            OAuthStateRepo::delete(&mut tx, result.id).await?;
        }
        tx.commit().await?;
        Ok(confirmed)
    }

    /// Stores the credential as a grant, connects each account to it, replaces the connected
    /// accounts' tokens, tidies the grants they left, and writes the audit events. The caller
    /// owns the transaction.
    ///
    /// A grant an account moved away from is deleted once unused, never revoked at the
    /// provider: it usually belongs to the same remote user and app as the new one, and some
    /// providers revoke every token of that pair together.
    pub(crate) async fn connect_in_tx(
        &self,
        tx: &mut PgConnection,
        scope: &OwnerScope,
        input: ConnectInput<'_>,
        audit: AuditContext,
    ) -> Result<Confirmed, AccountError> {
        let grant_id = GrantId::from(self.deps.ids.generate());
        let credential = self
            .deps
            .vault
            .seal(&input.secrets, &GrantRepo::seal_context(grant_id))?;
        GrantRepo::insert(
            tx,
            scope,
            NewGrant {
                id: grant_id,
                platform: input.platform.clone(),
                kind: input.kind,
                remote_user_id: input.remote_user_id,
                credential,
                connection_config: input.config,
                expires_at: input.expires_at,
                reconnect_by: input.reconnect_by,
                scopes: input.scopes.clone(),
            },
        )
        .await?;

        let mut connected = Vec::new();
        let mut skipped_elsewhere = 0;
        let mut previous_grants = Vec::new();
        for found in input.accounts {
            let new = NewAccount {
                id: AccountId::from(self.deps.ids.generate()),
                grant_id,
                platform: input.platform.clone(),
                remote_account_id: found.remote_id.clone(),
                display_name: input
                    .display_name
                    .map_or_else(|| found.display_name.clone(), str::to_owned),
                avatar_url: found.avatar_url.as_ref().map(ToString::to_string),
                scopes: input.scopes.clone(),
                test_account: false,
            };
            match AccountRepo::insert_or_reactivate(tx, scope, new).await {
                Ok(AccountUpsert::Created(account)) => {
                    connected.push((account, AccountChange::Created));
                }
                Ok(AccountUpsert::Reactivated(account)) => {
                    connected.push((account, AccountChange::Reactivated));
                }
                Ok(AccountUpsert::Moved {
                    account,
                    previous_grant,
                }) => {
                    previous_grants.push(previous_grant);
                    connected.push((account, AccountChange::Moved));
                }
                Err(DataError::AccountConnectedElsewhere) => skipped_elsewhere += 1,
                Err(error) => return Err(error.into()),
            }
        }

        previous_grants.sort();
        previous_grants.dedup();
        for previous in previous_grants {
            GrantRepo::delete_if_unused(tx, scope, previous).await?;
        }
        if connected.is_empty() {
            GrantRepo::delete_if_unused(tx, scope, grant_id).await?;
        } else {
            self.replace_account_tokens(tx, scope, &connected, &input.account_tokens)
                .await?;
        }

        for (account, change) in &connected {
            let event = AuditEvent::new(AuditEventKind::AccountConnected)
                .actor(scope.actor)
                .owner(scope.owner)
                .context(audit)
                .detail("account_id", account.id.to_string())
                .and_then(|e| e.detail("platform", account.platform.as_str()))
                .and_then(|e| e.detail("change", change.as_str()))
                .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
            AuditLog::record(tx, AuditEventId::from(self.deps.ids.generate()), event).await?;
        }
        Ok(Confirmed {
            connect: match input.kind {
                GrantKind::Oauth => ConnectKind::OAuth,
                GrantKind::Credentials => ConnectKind::Credentials,
            },
            connected,
            skipped_elsewhere,
            unknown: Vec::new(),
        })
    }
}

impl AccountService {
    /// `account_tokens` has no grant link, so every connected account (created, reactivated,
    /// or moved) first loses the token it had, then gets the new grant's token, if any.
    async fn replace_account_tokens(
        &self,
        tx: &mut PgConnection,
        scope: &OwnerScope,
        connected: &[(Account, AccountChange)],
        tokens: &BTreeMap<String, AccountToken>,
    ) -> Result<(), AccountError> {
        let ids: Vec<AccountId> = connected.iter().map(|(account, _)| account.id).collect();
        AccountTokenRepo::delete_for_accounts(tx, scope, &ids).await?;
        for (account, _) in connected {
            let Some(token) = tokens.get(&account.remote_account_id) else {
                continue;
            };
            let sealed = self.deps.vault.seal(
                &json!(token.access_token.expose_secret()),
                &AccountTokenRepo::seal_context(account.id),
            )?;
            AccountTokenRepo::upsert(tx, scope, account.id, &sealed, token.expires_at).await?;
        }
        Ok(())
    }
}

/// The input that moves an OAuth result's tokens into a new grant for `chosen`.
fn oauth_input<'a>(
    platform: &'a PlatformId,
    exchanged: StoredExchange,
    chosen: Vec<&'a DiscoveredAccount>,
) -> Result<ConnectInput<'a>, AccountError> {
    let secrets = serde_json::to_value(OAuthGrantCredential {
        access_token: exchanged.access_token,
        refresh_token: exchanged.refresh_token,
    })
    .map_err(|_| AccountError::Data(DataError::Conflict("grant tokens".to_owned())))?;
    let account_tokens = exchanged
        .account_tokens
        .into_iter()
        .map(|(remote_id, token)| {
            (
                remote_id,
                AccountToken {
                    access_token: SecretString::from(token.access_token),
                    expires_at: token.expires_at,
                },
            )
        })
        .collect();
    Ok(ConnectInput {
        platform,
        kind: GrantKind::Oauth,
        secrets,
        config: json!({}),
        remote_user_id: exchanged.remote_user_id,
        expires_at: exchanged.expires_at,
        reconnect_by: exchanged.reconnect_by,
        scopes: exchanged.scopes,
        account_tokens,
        accounts: chosen,
        display_name: None,
    })
}

/// The discovered accounts the user picked, and the picked IDs the credential did not reach.
/// `None` picks every discovered account.
pub(crate) fn choose(
    discovered: &[DiscoveredAccount],
    account_ids: Option<Vec<String>>,
) -> (Vec<&DiscoveredAccount>, Vec<String>) {
    let Some(ids) = account_ids else {
        return (discovered.iter().collect(), Vec::new());
    };
    let chosen = discovered
        .iter()
        .filter(|d| ids.contains(&d.remote_id))
        .collect();
    let mut unknown: Vec<String> = ids
        .into_iter()
        .filter(|id| !discovered.iter().any(|d| &d.remote_id == id))
        .collect();
    unknown.sort();
    unknown.dedup();
    (chosen, unknown)
}
