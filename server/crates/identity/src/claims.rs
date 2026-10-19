//! Turning verified claims into user rows.

use std::sync::Arc;

use postit_config::{BootstrapSettings, OidcClaimNames, UserinfoMode};
use postit_core::{AuditEventId, Clock, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::limits::UserLimitDefaults;
use postit_data::locks::{self, ADMIN_SET_LOCK_KEY};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::{ProvisionOutcome, UserRecord, UserStatus, UsersRepo};
use postit_http::HttpError;
use postit_i18n::Locale;
use postit_mail::MailOutbox;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::discovery::{JwksSource, OidcDiscovery};
use crate::error::IdentityError;
use crate::verifier::VerifiedClaims;

/// Static configuration for [`ClaimsTransformer`]: claim-name mapping, `userinfo` fetch
/// policy, the bootstrap-admin rule, and how often a coalesced approval email may go out.
#[derive(Debug, Clone)]
pub struct ClaimsConfig {
    /// Names of the claims to read from tokens.
    pub claim_names: OidcClaimNames,
    /// When to call the `userinfo` endpoint.
    pub userinfo_mode: UserinfoMode,
    /// Bootstrap-admin rule.
    pub bootstrap: BootstrapSettings,
    /// Minimum interval between approval emails to one admin.
    pub approval_email_interval: std::time::Duration,
    /// The limits a newly created user starts with.
    pub limits: UserLimitDefaults,
}

/// Turns a verified token's claims into a `users` row: cache-miss provisioning, the
/// bootstrap check, and profile-claim refresh including the `userinfo` fallback. Built
/// once per process and reused across requests — every field is `Clone`-cheap or an `Arc`.
#[derive(Clone)]
pub struct ClaimsTransformer<S: JwksSource> {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
    clock: Arc<dyn Clock>,
    http_client: reqwest::Client,
    discovery: Arc<OidcDiscovery<S>>,
    outbox: MailOutbox,
    config: ClaimsConfig,
}

impl<S: JwksSource> ClaimsTransformer<S> {
    /// A transformer over the given collaborators.
    #[must_use]
    pub fn new(
        pool: PgPool,
        ids: Arc<dyn IdGenerator>,
        clock: Arc<dyn Clock>,
        http_client: reqwest::Client,
        discovery: Arc<OidcDiscovery<S>>,
        outbox: MailOutbox,
        config: ClaimsConfig,
    ) -> Self {
        Self {
            pool,
            ids,
            clock,
            http_client,
            discovery,
            outbox,
            config,
        }
    }

    /// The discovery cache this transformer shares with the authenticator.
    #[must_use]
    pub fn discovery(&self) -> Arc<OidcDiscovery<S>> {
        Arc::clone(&self.discovery)
    }

    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] on a database failure. A failing `userinfo` call is
    /// not an error: the stored profile is kept (see [`Userinfo::Unavailable`]).
    pub async fn transform(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
        ctx: AuditContext,
        locale: Locale,
    ) -> Result<UserRecord, IdentityError> {
        let userinfo = self.fetch_userinfo_if_needed(verified, bearer_token).await;
        let userinfo_unavailable = matches!(userinfo, Userinfo::Unavailable);
        let userinfo_claims = match userinfo {
            Userinfo::Claims(map) => Some(map),
            Userinfo::NotRequested | Userinfo::Unavailable => None,
        };
        let lookup = |name: &str| -> Option<String> {
            userinfo_claims
                .as_ref()
                .and_then(|m| m.get(name))
                .and_then(Value::as_str)
                .or_else(|| claim_str(&verified.raw, name))
                .map(str::to_string)
        };
        let email = lookup(&self.config.claim_names.email);
        let verified_claim = userinfo_claims
            .as_ref()
            .and_then(|m| m.get(&self.config.claim_names.email_verified))
            .and_then(Value::as_bool)
            .or_else(|| claim_bool(&verified.raw, &self.config.claim_names.email_verified));
        let email_verified = verified_claim.unwrap_or(false);
        let name = lookup(&self.config.claim_names.name);
        let preferred_username = lookup(&self.config.claim_names.preferred_username);
        // `None` when the token carries no name claim, so a degraded `userinfo` can tell "the
        // token says nothing about the name" from "the name is the email or the subject".
        let named = name.or(preferred_username);
        let display_name = named
            .clone()
            .or_else(|| email.clone())
            .unwrap_or_else(|| verified.sub.clone());

        let mut tx = self.pool.begin().await.map_err(DataError::from)?;

        // Held for the whole transaction, not just the bootstrap check below: simpler than
        // conditionally locking, and sign-ins are infrequent enough (once per
        // auth.principal_cache_ttl per user, since this only runs on a cache miss) that the
        // extra serialization costs nothing that matters.
        locks::xact_lock(&mut tx, ADMIN_SET_LOCK_KEY)
            .await
            .map_err(IdentityError::from)?;

        let id = UserId::from(self.ids.generate());
        let (outcome, mut user) = UsersRepo::provision(
            &mut tx,
            id,
            &verified.iss,
            &verified.sub,
            &display_name,
            self.config.limits,
        )
        .await
        .map_err(IdentityError::from)?;

        if matches!(outcome, ProvisionOutcome::Created) {
            user = self
                .on_created(
                    &mut tx,
                    user,
                    verified,
                    (email.as_deref(), email_verified),
                    ctx,
                    locale,
                )
                .await?;
        }

        // A degraded `userinfo` says nothing about the profile, so an existing user is
        // refreshed per field from the token claims that are actually present and keeps the
        // stored value for every field the token does not carry.
        let (email, email_verified, display_name) =
            if userinfo_unavailable && !matches!(outcome, ProvisionOutcome::Created) {
                let merged_email = email.or_else(|| user.email.clone());
                let merged_verified = verified_claim.unwrap_or(
                    // Same address as stored: the stored flag still holds.
                    user.email_verified && merged_email == user.email,
                );
                let merged_name = named.unwrap_or_else(|| user.display_name.clone());
                (merged_email, merged_verified, merged_name)
            } else {
                (email, email_verified, display_name)
            };

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

    /// Runs once, right after `UsersRepo::provision` creates a new row: records the
    /// `user_provisioned` audit event, promotes the user to admin when they match the
    /// bootstrap rule and no active admin exists yet, and — if the user is still
    /// `pending` afterward — enqueues one coalesced approval email per active admin.
    async fn on_created(
        &self,
        tx: &mut sqlx::PgConnection,
        mut user: UserRecord,
        verified: &VerifiedClaims,
        (email, email_verified): (Option<&str>, bool),
        ctx: AuditContext,
        locale: Locale,
    ) -> Result<UserRecord, IdentityError> {
        UserPreferencesRepo::create_default(tx, user.id, "UTC", locale.tag())
            .await
            .map_err(IdentityError::from)?;

        let provisioned_audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            tx,
            provisioned_audit_id,
            AuditEvent::new(AuditEventKind::UserProvisioned)
                .context(ctx)
                .subject(user.id),
        )
        .await
        .map_err(IdentityError::from)?;

        // Bootstrap only ever considers a user this call just created — never an
        // existing one that starts matching the rule later. See Review Focus.
        if bootstrap_matches(&self.config.bootstrap, &verified.sub, email, email_verified) {
            let active_admins = UsersRepo::count_active_admins(tx)
                .await
                .map_err(IdentityError::from)?;
            if active_admins == 0 {
                user = UsersRepo::grant_admin(tx, user.id)
                    .await
                    .map_err(IdentityError::from)?;
                let bootstrap_audit_id = AuditEventId::from(self.ids.generate());
                AuditLog::record(
                    tx,
                    bootstrap_audit_id,
                    AuditEvent::new(AuditEventKind::BootstrapAdminGranted)
                        .context(ctx)
                        .subject(user.id),
                )
                .await
                .map_err(IdentityError::from)?;
            }
        }

        if user.status == UserStatus::Pending {
            crate::mail::enqueue_pending_approval_emails(
                tx,
                &self.outbox,
                self.config.approval_email_interval,
                self.clock.now(),
            )
            .await?;
        }

        Ok(user)
    }

    /// Calls `userinfo` when the policy asks for it. Every failure (discovery, transport,
    /// HTTP status, a body that is not JSON, a `sub` that is not the token's) is
    /// [`Userinfo::Unavailable`] and logged, never an error: sign-in goes on with the
    /// access-token claims and the stored profile. An `IdP` that advertises no endpoint is
    /// [`Userinfo::NotRequested`].
    async fn fetch_userinfo_if_needed(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Userinfo {
        let should_call = match self.config.userinfo_mode {
            UserinfoMode::Never => false,
            UserinfoMode::Always => true,
            UserinfoMode::Fallback => {
                claim_str(&verified.raw, &self.config.claim_names.email).is_none()
                    || claim_str(&verified.raw, &self.config.claim_names.name).is_none()
                    || claim_str(&verified.raw, &self.config.claim_names.preferred_username)
                        .is_none()
            }
        };
        if !should_call {
            return Userinfo::NotRequested;
        }
        match self.fetch_userinfo(verified, bearer_token).await {
            Ok(Some(claims)) => Userinfo::Claims(claims),
            // The IdP advertises no userinfo endpoint: nothing to call, so the token claims
            // are the whole story, exactly as when the policy asks for no call.
            Ok(None) => Userinfo::NotRequested,
            Err(err) => {
                tracing::warn!(error = %err, "userinfo unavailable; keeping the stored profile");
                Userinfo::Unavailable
            }
        }
    }

    async fn fetch_userinfo(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Result<Option<Map<String, Value>>, HttpError> {
        let Some(endpoint) = self.discovery.userinfo_endpoint().await? else {
            return Ok(None);
        };

        let request = self
            .http_client
            .get(&endpoint)
            .bearer_auth(bearer_token)
            .build()
            .map_err(|err| HttpError::Permanent(err.to_string()))?;
        let response = crate::discovery::ensure_success(
            postit_http::execute_traced(&self.http_client, request).await?,
        )?;
        let body: Value = response.json().await.map_err(HttpError::from)?;

        if body.get("sub").and_then(Value::as_str) != Some(verified.sub.as_str()) {
            // A userinfo response is used only when its sub matches the token's.
            return Err(HttpError::Permanent(
                "userinfo sub does not match the token's".to_string(),
            ));
        }
        Ok(body.as_object().cloned())
    }
}

/// What the `userinfo` step produced for one sign-in.
enum Userinfo {
    /// Policy did not ask for a call.
    NotRequested,
    /// The call was wanted but gave nothing usable.
    Unavailable,
    /// A response whose `sub` matched the token's.
    Claims(Map<String, Value>),
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
