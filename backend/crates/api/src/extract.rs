//! Request extractors. A handler takes exactly one of the authenticating ones, so the token
//! is verified once per request even though they share one implementation.

use std::net::IpAddr;

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use postit_core::UserId;
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::users::{UserRole, UserStatus};
use postit_i18n::negotiate;
use postit_identity::auth::{AuthError, ProvisionGate, RateLimited};
use postit_identity::delegation::resolve_act_as;
use postit_identity::principal::Principal;
use secrecy::{ExposeSecret, SecretString};

use crate::client_ip::ClientIp;
use crate::error::{ApiError, ErrorCode};
use crate::middleware::Authenticated;
use crate::state::AppState;

/// Longest bearer token accepted. Real access tokens are a few KiB; anything longer is
/// refused before it is base64-decoded and parsed.
pub const MAX_BEARER_LEN: usize = 8 * 1024;

/// The header a delegate sends to act in another user's workspace.
pub const ACT_AS: &str = "x-postit-act-as";

/// The length of a hyphenated UUID, the only form `X-Postit-Act-As` accepts.
const HYPHENATED_UUID_LEN: usize = 36;

/// Signed in, any status except `disabled` and `deleting` (so `pending` users reach `GET /me`).
/// Acts as the caller themselves: refuses `X-Postit-Act-As` with 403 `owner_only`.
pub struct Auth(pub Principal);
/// Signed in and `active`. Refuses `X-Postit-Act-As` with 403 `owner_only`.
pub struct ActiveUser(pub Principal);
/// Active admin. Checks the caller's own role only, and refuses `X-Postit-Act-As` with 403
/// `owner_only`.
pub struct RequireAdmin(pub Principal);
/// Where this request came from, for the audit events it causes: the trusted-proxy-resolved
/// [`ClientIp`] and the request ID. Never rejects, and is independent of [`Auth`], so a
/// handler takes it alongside any other extractor and passes `.0` into the service call that
/// writes the audit event.
pub struct RequestAudit(pub AuditContext);
/// The workspace the request acts in: the caller's own, or, with `X-Postit-Act-As`, the named
/// owner's under a live grant. The only extractor that accepts the header; every refusal of it
/// is the same detail-free 403 `delegation_invalid`.
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

fn audit_context(parts: &Parts) -> AuditContext {
    AuditContext {
        ip: parts.extensions.get::<ClientIp>().map(|c| c.0),
        // `sanitize_request_id` + `SetRequestIdLayer` guarantee a UUID; a missing or odd one
        // (a router built without them) records null rather than failing the request.
        request_id: parts
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| uuid::Uuid::parse_str(v).ok()),
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestAudit {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        std::future::ready(Ok(Self(audit_context(parts))))
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

/// Verifies the bearer token and refuses `disabled` and `deleting` accounts. Ignores
/// `X-Postit-Act-As`: the public extractors decide what it means.
async fn authenticated(parts: &Parts, state: &AppState) -> Result<Principal, ApiError> {
    let ip = parts
        .extensions
        .get::<ClientIp>()
        .map_or(IpAddr::from([0, 0, 0, 0]), |c| c.0);
    let Some(token) = bearer(parts) else {
        return Err(ApiError::missing_token());
    };
    if token.expose_secret().len() > MAX_BEARER_LEN {
        // Charged to the IP bucket like any other bad token (never marked authenticated).
        return Err(ApiError::new(ErrorCode::Unauthenticated));
    }
    let locale = negotiate(
        parts
            .headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok()),
    );
    let principal = match state
        .auth
        .authenticate(&token, &IpGate { state, ip }, audit_context(parts), locale)
        .await
    {
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
    Ok(principal)
}

/// [`authenticated`] and `active` (today's [`ActiveUser`] without the act-as refusal, so
/// [`Scope`] can build on it).
async fn active_principal(parts: &Parts, state: &AppState) -> Result<Principal, ApiError> {
    let principal = authenticated(parts, state).await?;
    if principal.status == UserStatus::Pending {
        return Err(ApiError::new(ErrorCode::AccountPending));
    }
    Ok(principal)
}

/// Refuses `X-Postit-Act-As` on a route that acts as the caller themselves: honoring it is
/// impossible, and ignoring it would let a client believe it acted in someone else's
/// workspace. Runs after authentication, so a missing or bad token is still 401.
fn refuse_act_as(parts: &Parts) -> Result<(), ApiError> {
    if parts.headers.contains_key(ACT_AS) {
        return Err(ApiError::new(ErrorCode::OwnerOnly));
    }
    Ok(())
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = authenticated(parts, state).await?;
        refuse_act_as(parts)?;
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for ActiveUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = active_principal(parts, state).await?;
        refuse_act_as(parts)?;
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = active_principal(parts, state).await?;
        refuse_act_as(parts)?;
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
        let principal = active_principal(parts, state).await?;
        let mut values = parts.headers.get_all(ACT_AS).iter();
        let Some(value) = values.next() else {
            return Ok(Self(OwnerScope::own(principal.user_id)));
        };
        // Every refusal is the same detail-free 403, so the header cannot probe which user
        // IDs exist or which grants are live.
        let invalid = || ApiError::new(ErrorCode::DelegationInvalid);
        if values.next().is_some() {
            return Err(invalid());
        }
        let owner = act_as_owner(value.as_bytes()).ok_or_else(invalid)?;
        resolve_act_as(&state.pool, principal.user_id, owner, state.clock.now())
            .await?
            .map(Self)
            .ok_or_else(invalid)
    }
}

/// The owner a raw `X-Postit-Act-As` value names: a hyphenated UUID in either case, taken as
/// sent (no trimming). The length check refuses the braced, `urn:uuid:`, and unhyphenated
/// forms that `Uuid::try_parse` would otherwise accept.
fn act_as_owner(raw: &[u8]) -> Option<UserId> {
    if raw.len() != HYPHENATED_UUID_LEN {
        return None;
    }
    let text = std::str::from_utf8(raw).ok()?;
    uuid::Uuid::try_parse(text).ok().map(UserId::from)
}

#[cfg(test)]
mod tests {
    use super::act_as_owner;

    #[test]
    fn act_as_accepts_only_the_hyphenated_form() {
        let id = "0190f000-0000-7000-8000-00000000000a";
        assert!(act_as_owner(id.as_bytes()).is_some());
        assert!(act_as_owner(id.to_uppercase().as_bytes()).is_some());
        for bad in [
            String::new(),
            format!(" {id}"),
            format!("{id} "),
            format!("{{{id}}}"),
            format!("urn:uuid:{id}"),
            id.replace('-', ""),
            "0190f000-0000-7000-8000-00000000000g".to_owned(),
            "0190f000_0000_7000_8000_00000000000a".to_owned(),
        ] {
            assert!(act_as_owner(bad.as_bytes()).is_none(), "{bad:?}");
        }
    }
}
