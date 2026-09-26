use std::sync::Arc;

use postit_config::{BootstrapSettings, OidcClaimNames, UserinfoMode};
use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::locks::{self, BOOTSTRAP_ADMIN_LOCK_KEY};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::{ProvisionOutcome, UserRecord, UsersRepo};
use postit_http::HttpError;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::discovery::{JwksSource, OidcDiscovery};
use crate::error::IdentityError;
use crate::verifier::VerifiedClaims;

/// Turns a verified token's claims into a `users` row: cache-miss provisioning, the
/// bootstrap check, and profile-claim refresh including the `userinfo` fallback. Built
/// once per process and reused across requests — every field is `Clone`-cheap or an `Arc`.
#[derive(Clone)]
pub struct ClaimsTransformer<S: JwksSource> {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
    http_client: reqwest::Client,
    discovery: Arc<OidcDiscovery<S>>,
    claim_names: OidcClaimNames,
    userinfo_mode: UserinfoMode,
    bootstrap: BootstrapSettings,
}

impl<S: JwksSource> ClaimsTransformer<S> {
    #[must_use]
    pub fn new(
        pool: PgPool,
        ids: Arc<dyn IdGenerator>,
        http_client: reqwest::Client,
        discovery: Arc<OidcDiscovery<S>>,
        claim_names: OidcClaimNames,
        userinfo_mode: UserinfoMode,
        bootstrap: BootstrapSettings,
    ) -> Self {
        Self {
            pool,
            ids,
            http_client,
            discovery,
            claim_names,
            userinfo_mode,
            bootstrap,
        }
    }

    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] on a database failure or
    /// [`IdentityError::Http`] if a required `userinfo` call fails.
    pub async fn transform(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Result<UserRecord, IdentityError> {
        let userinfo_claims = self
            .fetch_userinfo_if_needed(verified, bearer_token)
            .await?;
        let lookup = |name: &str| -> Option<String> {
            userinfo_claims
                .as_ref()
                .and_then(|m| m.get(name))
                .and_then(Value::as_str)
                .or_else(|| claim_str(&verified.raw, name))
                .map(str::to_string)
        };
        let email = lookup(&self.claim_names.email);
        let email_verified = userinfo_claims
            .as_ref()
            .and_then(|m| m.get(&self.claim_names.email_verified))
            .and_then(Value::as_bool)
            .or_else(|| claim_bool(&verified.raw, &self.claim_names.email_verified))
            .unwrap_or(false);
        let name = lookup(&self.claim_names.name);
        let preferred_username = lookup(&self.claim_names.preferred_username);
        let display_name = name
            .or(preferred_username)
            .or_else(|| email.clone())
            .unwrap_or_else(|| verified.sub.clone());

        let mut tx = self.pool.begin().await.map_err(DataError::from)?;

        // Held for the whole transaction, not just the bootstrap check below: simpler than
        // conditionally locking, and sign-ins are infrequent enough (once per
        // auth.principal_cache_ttl per user, since this only runs on a cache miss) that the
        // extra serialization costs nothing that matters.
        locks::xact_lock(&mut tx, BOOTSTRAP_ADMIN_LOCK_KEY)
            .await
            .map_err(IdentityError::from)?;

        let id = UserId::from(self.ids.generate());
        let (outcome, mut user) =
            UsersRepo::provision(&mut tx, id, &verified.iss, &verified.sub, &display_name)
                .await
                .map_err(IdentityError::from)?;

        if matches!(outcome, ProvisionOutcome::Created) {
            UserPreferencesRepo::create_default(&mut tx, user.id, "UTC")
                .await
                .map_err(IdentityError::from)?;

            let provisioned_audit_id = AuditEventId::from(self.ids.generate());
            AuditLog::record(
                &mut tx,
                provisioned_audit_id,
                AuditEvent::new(AuditEventKind::UserProvisioned).subject(user.id),
            )
            .await
            .map_err(IdentityError::from)?;

            // Bootstrap only ever considers a user this call just created — never an
            // existing one that starts matching the rule later. See Review Focus.
            if bootstrap_matches(
                &self.bootstrap,
                &verified.sub,
                email.as_deref(),
                email_verified,
            ) {
                let active_admins = UsersRepo::count_active_admins(&mut tx)
                    .await
                    .map_err(IdentityError::from)?;
                if active_admins == 0 {
                    user = UsersRepo::grant_admin(&mut tx, user.id)
                        .await
                        .map_err(IdentityError::from)?;
                    let bootstrap_audit_id = AuditEventId::from(self.ids.generate());
                    AuditLog::record(
                        &mut tx,
                        bootstrap_audit_id,
                        AuditEvent::new(AuditEventKind::BootstrapAdminGranted).subject(user.id),
                    )
                    .await
                    .map_err(IdentityError::from)?;
                }
            }
        }

        // `UsersRepo::provision` only ever sets `display_name` on insert (email and
        // email_verified start `NULL`/`false`), so a newly created user still needs this
        // same profile-claims write to pick up the email captured above; an existing user
        // needs it whenever the claims changed since last sign-in. Re-reading `user.id`
        // instead of trusting `outcome` alone keeps this correct even after the bootstrap
        // branch above replaced `user` with the promoted row.
        if user.email.as_deref() != email.as_deref()
            || user.email_verified != email_verified
            || user.display_name != display_name
        {
            UsersRepo::update_profile_claims(
                &mut tx,
                user.id,
                email.as_deref(),
                email_verified,
                &display_name,
            )
            .await
            .map_err(IdentityError::from)?;
            user = UsersRepo::find_by_id(&mut tx, user.id)
                .await
                .map_err(IdentityError::from)?
                .ok_or(DataError::NotFound)
                .map_err(IdentityError::from)?;
        }

        tx.commit().await.map_err(DataError::from)?;
        Ok(user)
    }

    async fn fetch_userinfo_if_needed(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Result<Option<Map<String, Value>>, IdentityError> {
        let should_call = match self.userinfo_mode {
            UserinfoMode::Never => false,
            UserinfoMode::Always => true,
            UserinfoMode::Fallback => {
                claim_str(&verified.raw, &self.claim_names.email).is_none()
                    || claim_str(&verified.raw, &self.claim_names.name).is_none()
                    || claim_str(&verified.raw, &self.claim_names.preferred_username).is_none()
            }
        };
        if !should_call {
            return Ok(None);
        }

        let Some(endpoint) = self
            .discovery
            .userinfo_endpoint()
            .await
            .map_err(IdentityError::from)?
        else {
            return Ok(None);
        };

        let request = self
            .http_client
            .get(&endpoint)
            .bearer_auth(bearer_token)
            .build()
            .map_err(|err| IdentityError::Http(HttpError::Permanent(err.to_string())))?;
        let response = postit_http::execute_traced(&self.http_client, request)
            .await
            .map_err(IdentityError::from)?;
        let body: Value = response
            .json()
            .await
            .map_err(|err| IdentityError::Http(HttpError::from(err)))?;

        if body.get("sub").and_then(Value::as_str) != Some(verified.sub.as_str()) {
            // A userinfo response is used only when its sub matches the token's.
            return Ok(None);
        }
        Ok(body.as_object().cloned())
    }
}

fn claim_str<'a>(raw: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    raw.get(name).and_then(Value::as_str)
}

fn claim_bool(raw: &Map<String, Value>, name: &str) -> Option<bool> {
    raw.get(name).and_then(Value::as_bool)
}

fn bootstrap_matches(
    bootstrap: &BootstrapSettings,
    subject: &str,
    email: Option<&str>,
    email_verified: bool,
) -> bool {
    if let Some(admin_subject) = &bootstrap.admin_subject {
        return admin_subject == subject;
    }
    if let Some(admin_email) = &bootstrap.admin_email {
        return email_verified && email.is_some_and(|e| e.eq_ignore_ascii_case(admin_email));
    }
    false
}
