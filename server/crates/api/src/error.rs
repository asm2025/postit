//! RFC 9457 problem+json. Handlers return `ApiError`; `render_problems` writes the body so it
//! can include the request ID, and also turns bare 404/408/413 responses from the router and
//! tower-http layers into problems.

use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use postit_data::DataError;
use postit_identity::IdentityError;
use postit_identity::auth::AuthError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthenticated,
    AccountPending,
    AccountDisabled,
    Forbidden,
    NotFound,
    RequestTimeout,
    UserDeleting,
    LastAdmin,
    IdempotencyInProgress,
    VersionConflict,
    PayloadTooLarge,
    ValidationFailed,
    IdempotencyKeyReused,
    PreconditionRequired,
    RateLimited,
    Internal,
    Unavailable,
}

impl ErrorCode {
    pub const ALL: [Self; 17] = [
        Self::Unauthenticated,
        Self::AccountPending,
        Self::AccountDisabled,
        Self::Forbidden,
        Self::NotFound,
        Self::RequestTimeout,
        Self::UserDeleting,
        Self::LastAdmin,
        Self::IdempotencyInProgress,
        Self::VersionConflict,
        Self::PayloadTooLarge,
        Self::ValidationFailed,
        Self::IdempotencyKeyReused,
        Self::PreconditionRequired,
        Self::RateLimited,
        Self::Internal,
        Self::Unavailable,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::AccountPending => "account_pending",
            Self::AccountDisabled => "account_disabled",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::RequestTimeout => "request_timeout",
            Self::UserDeleting => "user_deleting",
            Self::LastAdmin => "last_admin",
            Self::IdempotencyInProgress => "idempotency_in_progress",
            Self::VersionConflict => "version_conflict",
            Self::PayloadTooLarge => "payload_too_large",
            Self::ValidationFailed => "validation_failed",
            Self::IdempotencyKeyReused => "idempotency_key_reused",
            Self::PreconditionRequired => "precondition_required",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
        }
    }

    #[must_use]
    pub fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::AccountPending | Self::AccountDisabled | Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::UserDeleting | Self::LastAdmin | Self::IdempotencyInProgress => {
                StatusCode::CONFLICT
            }
            Self::VersionConflict => StatusCode::PRECONDITION_FAILED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::ValidationFailed | Self::IdempotencyKeyReused => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Unauthenticated => "Authentication required",
            Self::AccountPending => "Account pending approval",
            Self::AccountDisabled => "Account disabled",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not found",
            Self::RequestTimeout => "Request timed out",
            Self::UserDeleting => "User is being deleted",
            Self::LastAdmin => "Last active admin",
            Self::IdempotencyInProgress => "Request with this key is in progress",
            Self::VersionConflict => "Version conflict",
            Self::PayloadTooLarge => "Payload too large",
            Self::ValidationFailed => "Validation failed",
            Self::IdempotencyKeyReused => "Idempotency key reused",
            Self::PreconditionRequired => "Precondition required",
            Self::RateLimited => "Too many requests",
            Self::Internal => "Internal error",
            Self::Unavailable => "Service unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub code: ErrorCode,
    pub detail: Option<String>,
    pub retry_after: Option<Duration>,
    /// `unauthenticated` because the request carried no usable bearer token at all, as
    /// opposed to one that failed verification. RFC 6750 §3.1: the challenge then carries no
    /// `error` code.
    pub token_missing: bool,
}

// Clone: axum's closure `Handler` impl requires `Clone`, so tests (and any handler that
// returns a captured error) need it; `Problem` is already `Clone`.
#[derive(Debug, Clone)]
pub struct ApiError(pub Problem);

impl ApiError {
    #[must_use]
    pub fn new(code: ErrorCode) -> Self {
        Self(Problem {
            code,
            detail: None,
            retry_after: None,
            token_missing: false,
        })
    }

    /// `unauthenticated` for a request with no usable bearer token (no `Authorization`
    /// header, another scheme, or an empty token): `WWW-Authenticate: Bearer`, no error code.
    #[must_use]
    pub fn missing_token() -> Self {
        let mut err = Self::new(ErrorCode::Unauthenticated);
        err.0.token_missing = true;
        err
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.0.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn with_retry_after(mut self, retry_after: Duration) -> Self {
        self.0.retry_after = Some(retry_after);
        self
    }

    /// Logs `err` and returns a detail-free 500.
    #[must_use]
    pub fn internal(err: &dyn std::fmt::Display) -> Self {
        tracing::error!(error = %err, "internal error");
        Self::new(ErrorCode::Internal)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = self.0.code.status().into_response();
        // An internal error never carries detail, whatever the caller attached.
        let mut problem = self.0;
        if problem.code == ErrorCode::Internal {
            problem.detail = None;
        }
        response.extensions_mut().insert(problem);
        response
    }
}

impl From<DataError> for ApiError {
    fn from(err: DataError) -> Self {
        match err {
            DataError::NotFound => Self::new(ErrorCode::NotFound),
            other => Self::internal(&other),
        }
    }
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        match err {
            IdentityError::InvalidTransition(from, to) => Self::new(ErrorCode::ValidationFailed)
                .with_detail(format!("status transition not allowed: {from} -> {to}")),
            IdentityError::LastAdmin => Self::new(ErrorCode::LastAdmin),
            IdentityError::UserDeleting => Self::new(ErrorCode::UserDeleting),
            IdentityError::CannotDeleteSelf => Self::new(ErrorCode::Forbidden)
                .with_detail("use DELETE /api/v1/me to delete your own account"),
            IdentityError::Data(DataError::NotFound) => Self::new(ErrorCode::NotFound),
            other => Self::internal(&other),
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Invalid(_) => Self::new(ErrorCode::Unauthenticated),
            AuthError::JwksUnavailable => {
                Self::new(ErrorCode::Unavailable).with_detail("signing keys are not loaded yet")
            }
            AuthError::RateLimited(limited) => {
                Self::new(ErrorCode::RateLimited).with_retry_after(limited.retry_after)
            }
            AuthError::Internal(inner) => Self::internal(&inner),
        }
    }
}

/// Outermost-but-one middleware (inside the request-ID layer): writes the problem body for
/// any response carrying a [`Problem`] extension, and converts bare 404/408/413 responses.
pub async fn render_problems(req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut response = next.run(req).await;
    let problem = response.extensions_mut().remove::<Problem>().or_else(|| {
        let bare = response.headers().get(header::CONTENT_TYPE).is_none();
        let code = match response.status() {
            StatusCode::NOT_FOUND if bare => Some(ErrorCode::NotFound),
            StatusCode::REQUEST_TIMEOUT => Some(ErrorCode::RequestTimeout),
            StatusCode::PAYLOAD_TOO_LARGE => Some(ErrorCode::PayloadTooLarge),
            _ => None,
        };
        code.map(|code| Problem {
            code,
            detail: None,
            retry_after: None,
            token_missing: false,
        })
    });
    let Some(problem) = problem else {
        return response;
    };

    let body = serde_json::json!({
        "type": "about:blank",
        "title": problem.code.title(),
        "status": problem.code.status().as_u16(),
        "code": problem.code.as_str(),
        "request_id": request_id,
        "detail": problem.detail,
    });
    let (mut parts, _) = response.into_parts();
    parts.status = problem.code.status();
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    parts.headers.remove(header::CONTENT_LENGTH);
    if problem.code == ErrorCode::Unauthenticated {
        // RFC 6750 §3.1: no error code when the request had no credentials at all.
        let challenge = if problem.token_missing {
            "Bearer"
        } else {
            "Bearer error=\"invalid_token\""
        };
        parts.headers.insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(challenge),
        );
    }
    if let Some(retry_after) = problem.retry_after {
        // Rounded up: a client that waits the advertised seconds must find a token ready.
        let secs = (retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0)).max(1);
        if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
            parts.headers.insert(header::RETRY_AFTER, value);
        }
    }
    Response::from_parts(parts, Body::from(body.to_string()))
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::routing::get;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    async fn render(err: ApiError) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let app = Router::new()
            .route("/", get(move || async move { err }))
            .layer(axum::middleware::from_fn(render_problems));
        let req = axum::http::Request::builder()
            .uri("/")
            .header("x-request-id", "0190d5a6-0000-7000-8000-000000000001")
            .body(Body::empty())
            .unwrap_or_else(|e| unreachable!("request: {e}"));
        let res = app
            .oneshot(req)
            .await
            .unwrap_or_else(|e| unreachable!("oneshot: {e}"));
        let (parts, body) = res.into_parts();
        let bytes = body
            .collect()
            .await
            .unwrap_or_else(|e| unreachable!("body: {e}"))
            .to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (parts.status, parts.headers, json)
    }

    #[tokio::test]
    async fn every_code_renders_problem_json_with_request_id() {
        for code in ErrorCode::ALL {
            let (status, headers, body) = render(ApiError::new(code)).await;
            assert_eq!(status, code.status());
            assert_eq!(
                headers
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
                Some("application/problem+json")
            );
            assert_eq!(body["code"], code.as_str());
            assert_eq!(body["request_id"], "0190d5a6-0000-7000-8000-000000000001");
        }
    }

    #[tokio::test]
    async fn unauthenticated_carries_www_authenticate() {
        let (_, headers, _) = render(ApiError::new(ErrorCode::Unauthenticated)).await;
        assert_eq!(
            headers
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer error=\"invalid_token\"")
        );
        let (status, headers, body) = render(ApiError::missing_token()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["code"], "unauthenticated");
        assert_eq!(
            headers
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer"),
            "no error code without credentials (RFC 6750 §3.1)"
        );
    }

    #[tokio::test]
    async fn rate_limited_carries_retry_after() {
        let err =
            ApiError::new(ErrorCode::RateLimited).with_retry_after(Duration::from_millis(2500));
        let (_, headers, _) = render(err).await;
        assert_eq!(
            headers
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("3")
        );
    }

    #[tokio::test]
    async fn internal_never_exposes_the_underlying_error() {
        let err = ApiError::from(DataError::Conflict("SELECT secret FROM vault".into()));
        let (status, _, body) = render(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["detail"].is_null());
        assert!(!body.to_string().contains("vault"));
    }
}
