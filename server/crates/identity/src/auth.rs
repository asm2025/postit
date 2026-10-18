//! The token to `Principal` pipeline. `postit-api`'s `Auth` extractor calls
//! [`Authenticate::authenticate`] and maps [`AuthError`]; everything identity-specific
//! (JWKS, verification, the principal cache, provisioning) stays here.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::UserId;
use postit_data::DataError;
use postit_data::audit::AuditContext;
use postit_data::users::UsersRepo;
use postit_http::HttpError;
use postit_i18n::Locale;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

use crate::cache::PrincipalCache;
use crate::claims::ClaimsTransformer;
use crate::discovery::{DiscoveryDocument, JwksSource, OidcDiscovery};
use crate::error::{IdentityError, VerifyError};
use crate::principal::Principal;
use crate::verifier::Verifier;

/// The caller must wait before provisioning again.
#[derive(Debug, Clone, Copy)]
pub struct RateLimited {
    /// How long to wait.
    pub retry_after: Duration,
}

/// Asked once, just before a request would create a `users` row, so the API can apply its
/// stricter provisioning rate limit (keyed by client IP) without knowing identity internals.
pub trait ProvisionGate: Send + Sync {
    /// # Errors
    ///
    /// Returns [`RateLimited`] when the caller must not provision right now.
    fn check(&self) -> Result<(), RateLimited>;
}

/// A gate that never refuses: tests and callers without an HTTP client IP.
pub struct AllowAll;

impl ProvisionGate for AllowAll {
    fn check(&self) -> Result<(), RateLimited> {
        Ok(())
    }
}

/// Why authentication failed.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The token failed verification.
    #[error("invalid token: {0}")]
    Invalid(VerifyError),
    /// The signing keys have not been fetched yet.
    #[error("signing keys are not available yet")]
    JwksUnavailable,
    /// A first sign-in was refused by the provisioning rate limit.
    #[error("provisioning rate limited")]
    RateLimited(RateLimited),
    /// An unexpected failure resolving the user.
    #[error(transparent)]
    Internal(IdentityError),
}

/// Turns a bearer token into the [`Principal`] it identifies.
#[async_trait]
pub trait Authenticate: Send + Sync {
    /// # Errors
    ///
    /// See [`AuthError`]. Status (pending, disabled, deleting) is not checked here. `ctx` is
    /// recorded on the audit events a first sign-in writes (provisioning, bootstrap). `locale`
    /// is stored only when this sign-in creates the user; an existing user's locale is kept.
    async fn authenticate(
        &self,
        bearer: &SecretString,
        gate: &dyn ProvisionGate,
        ctx: AuditContext,
        locale: Locale,
    ) -> Result<Principal, AuthError>;

    /// Loads the JWKS (and discovery document) if nothing is cached yet.
    ///
    /// # Errors
    ///
    /// Returns [`HttpError`] if the fetch fails.
    async fn prefetch(&self) -> Result<(), HttpError>;

    /// Whether the signing keys are loaded, for `/ready`.
    fn jwks_ready(&self) -> bool;

    /// The cached discovery document, if loaded.
    fn discovery_document(&self) -> Option<DiscoveryDocument>;

    /// Evicts `user` from this process's principal cache. Admin changes call it after
    /// committing (plan 01, "Cache invalidation across processes"); the `pg_notify` those
    /// changes send covers every other process.
    fn invalidate_user(&self, user: UserId);
}

/// The collaborators an [`Authenticator`] is built from.
pub struct AuthenticatorParts<S: JwksSource> {
    /// Verifies token signatures and claims.
    pub verifier: Verifier,
    /// Source of signing keys and the discovery document.
    pub discovery: Arc<OidcDiscovery<S>>,
    /// Cache of resolved principals.
    pub cache: PrincipalCache,
    /// Resolves verified claims to a user row.
    pub transformer: ClaimsTransformer<S>,
    /// Database pool.
    pub pool: PgPool,
}

/// The production [`Authenticate`] implementation.
pub struct Authenticator<S: JwksSource> {
    parts: AuthenticatorParts<S>,
}

impl<S: JwksSource + 'static> Authenticator<S> {
    /// An authenticator over `parts`.
    #[must_use]
    pub fn new(parts: AuthenticatorParts<S>) -> Self {
        Self { parts }
    }
}

fn internal_data(err: DataError) -> AuthError {
    AuthError::Internal(err.into())
}

#[async_trait]
impl<S: JwksSource + 'static> Authenticate for Authenticator<S> {
    #[tracing::instrument(skip_all)]
    async fn authenticate(
        &self,
        bearer: &SecretString,
        gate: &dyn ProvisionGate,
        ctx: AuditContext,
        locale: Locale,
    ) -> Result<Principal, AuthError> {
        let p = &self.parts;
        let token = bearer.expose_secret();
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| AuthError::Invalid(VerifyError::Malformed))?;
        let kid = header
            .kid
            .ok_or(AuthError::Invalid(VerifyError::UnknownKid))?;
        let jwks = p
            .discovery
            .jwks_for_kid(&kid)
            .await
            .map_err(|_| AuthError::JwksUnavailable)?;
        let verified = p
            .verifier
            .verify(token, &jwks)
            .map_err(AuthError::Invalid)?;

        if let Some(principal) = p.cache.get(&verified.iss, &verified.sub) {
            return Ok(principal);
        }

        let generation = p.cache.generation();
        let mut conn = p
            .pool
            .acquire()
            .await
            .map_err(|e| internal_data(e.into()))?;
        let existing = UsersRepo::find_by_oidc(&mut conn, &verified.iss, &verified.sub)
            .await
            .map_err(internal_data)?;
        drop(conn);
        if existing.is_none() {
            gate.check().map_err(AuthError::RateLimited)?;
        }
        let record = p
            .transformer
            .transform(&verified, token, ctx, locale)
            .await
            .map_err(AuthError::Internal)?;
        let mut conn = p
            .pool
            .acquire()
            .await
            .map_err(|e| internal_data(e.into()))?;
        UsersRepo::touch_last_seen(&mut conn, record.id)
            .await
            .map_err(internal_data)?;
        drop(conn);

        let principal = Principal::from(&record);
        let _ = p
            .cache
            .insert_if_current(generation, &verified.iss, &verified.sub, principal);
        Ok(principal)
    }

    async fn prefetch(&self) -> Result<(), HttpError> {
        self.parts.discovery.jwks().await.map(|_| ())
    }

    fn jwks_ready(&self) -> bool {
        self.parts.discovery.is_loaded()
    }

    fn discovery_document(&self) -> Option<DiscoveryDocument> {
        self.parts.discovery.document()
    }

    fn invalidate_user(&self, user: UserId) {
        self.parts.cache.invalidate_user(user);
    }
}
