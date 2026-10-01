//! Request extractors. A handler takes exactly one of them, so `Auth` runs once per request
//! even though the others delegate to it.

use std::net::IpAddr;

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use postit_data::OwnerScope;
use postit_data::users::{UserRole, UserStatus};
use postit_identity::auth::{AuthError, ProvisionGate, RateLimited};
use postit_identity::principal::Principal;
use secrecy::SecretString;

use crate::client_ip::ClientIp;
use crate::error::{ApiError, ErrorCode};
use crate::middleware::Authenticated;
use crate::state::AppState;

/// Signed in, any status except `disabled` and `deleting` (so `pending` users reach `GET /me`).
pub struct Auth(pub Principal);
/// Signed in and `active`.
pub struct ActiveUser(pub Principal);
/// Active admin. Checks the caller's own role only.
pub struct RequireAdmin(pub Principal);
/// The caller's own workspace. Plan 03 adds `X-Postit-Act-As` here and nowhere else.
pub struct Scope(pub OwnerScope);

struct IpGate<'a> {
    state: &'a AppState,
    ip: IpAddr,
}

impl ProvisionGate for IpGate<'_> {
    fn check(&self) -> Result<(), RateLimited> {
        self.state
            .limits
            .provisioning
            .check(&self.ip)
            .map_err(|retry_after| RateLimited { retry_after })
    }
}

fn bearer(parts: &Parts) -> Option<SecretString> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| SecretString::from(token.to_owned()))
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ClientIp>()
            .map_or(IpAddr::from([0, 0, 0, 0]), |c| c.0);
        let Some(token) = bearer(parts) else {
            return Err(ApiError::missing_token());
        };
        let principal = match state.auth.authenticate(&token, &IpGate { state, ip }).await {
            Ok(principal) => principal,
            Err(AuthError::Invalid(err)) => {
                // `err` is a VerifyError kind (expired, bad signature, ...), never token text.
                tracing::debug!(error = %err, "bearer token rejected");
                // Not charged here: `ip_rate_limit` charges every request that ends without
                // `Authenticated` being marked, including this one.
                return Err(ApiError::new(ErrorCode::Unauthenticated));
            }
            Err(other) => return Err(other.into()),
        };
        // The token is genuine: from here on this request is charged per user, not per IP,
        // whatever the outcome (a disabled account is still a real, authenticated caller).
        if let Some(outcome) = parts.extensions.get::<Authenticated>() {
            outcome.mark();
        }
        // Charged before the status check, so a disabled account's requests are bounded too.
        if let Err(wait) = state.limits.user.check(&principal.user_id) {
            return Err(ApiError::new(ErrorCode::RateLimited).with_retry_after(wait));
        }
        if matches!(
            principal.status,
            UserStatus::Disabled | UserStatus::Deleting
        ) {
            return Err(ApiError::new(ErrorCode::AccountDisabled));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for ActiveUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Auth(principal) = Auth::from_request_parts(parts, state).await?;
        if principal.status == UserStatus::Pending {
            return Err(ApiError::new(ErrorCode::AccountPending));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ActiveUser(principal) = ActiveUser::from_request_parts(parts, state).await?;
        if principal.role != UserRole::Admin {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for Scope {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ActiveUser(principal) = ActiveUser::from_request_parts(parts, state).await?;
        Ok(Self(OwnerScope::own(principal.user_id)))
    }
}
