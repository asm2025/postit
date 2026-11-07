//! `OpenAPI` responses every authenticated route can answer, so each handler's `responses(...)`
//! lists only what is specific to it.

use utoipa::IntoResponses;

use crate::error::ProblemDetails;

/// The problems any authenticated route can answer: an unusable token, a spent rate limit, an
/// internal failure, and a dependency that is not ready.
#[derive(IntoResponses)]
#[allow(dead_code)]
pub enum CommonProblems {
    /// No valid bearer token (`unauthenticated`).
    #[response(status = 401, content_type = "application/problem+json")]
    Unauthenticated(#[to_schema] ProblemDetails),
    /// `rate_limited`; see `Retry-After`.
    #[response(status = 429, content_type = "application/problem+json")]
    RateLimited(#[to_schema] ProblemDetails),
    /// `internal`.
    #[response(status = 500, content_type = "application/problem+json")]
    Internal(#[to_schema] ProblemDetails),
    /// `unavailable` (signing keys not loaded yet).
    #[response(status = 503, content_type = "application/problem+json")]
    Unavailable(#[to_schema] ProblemDetails),
}

/// The 403 every route behind a workspace scope can answer.
#[derive(IntoResponses)]
#[allow(dead_code)]
pub enum ScopeProblems {
    /// `account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no
    /// workspace the caller holds a live grant to), `owner_only` (an owner-only action through
    /// `X-Postit-Act-As`), or `capability_required` (the grant's level does not allow it).
    #[response(status = 403, content_type = "application/problem+json")]
    Forbidden(#[to_schema] ProblemDetails),
}
