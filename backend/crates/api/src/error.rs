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
use postit_identity::delegation::DelegationError;
use utoipa::ToSchema;

/// Machine-readable problem code (the `code` member of every problem+json body).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[schema(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 401: no valid bearer token.
    Unauthenticated,
    /// 403: the account is waiting for approval.
    AccountPending,
    /// 403: the account is disabled.
    AccountDisabled,
    /// 403: the caller lacks the required role.
    Forbidden,
    /// 403: only the workspace owner may do this.
    OwnerOnly,
    /// 403: the delegation does not allow this action.
    CapabilityRequired,
    /// 403: `X-Postit-Act-As` does not name a workspace the caller may act in.
    DelegationInvalid,
    /// 403: the action reaches accounts outside the caller's delegation limit.
    AccountNotInScope,
    /// 404: no such resource, or not visible to the caller.
    NotFound,
    /// 404: push notifications are off in this deployment.
    PushDisabled,
    /// 408: the request took too long.
    RequestTimeout,
    /// 409: the user is being deleted.
    UserDeleting,
    /// 409: the change would leave no active admin.
    LastAdmin,
    /// 409: a request with the same idempotency key is still running.
    IdempotencyInProgress,
    /// 409: the job action does not apply to the job in its current state.
    JobActionNotAllowed,
    /// 409: an admin switched the platform off.
    PlatformDisabled,
    /// 409: the platform does not offer this way of connecting.
    CapabilityUnsupported,
    /// 409: the owner already has an email sending method.
    SendingMethodExists,
    /// 409: the owner already has a live delegation to this user.
    DelegationExists,
    /// 409: a post is being published to the account right now.
    JobActive,
    /// 409: a post or an unfinished job still uses the media.
    MediaInUse,
    /// 409: the chunk does not start where the stored bytes end, or the upload is not whole yet.
    UploadOffsetMismatch,
    /// 412: the `If-Match` version is stale.
    VersionConflict,
    /// 413: the request body exceeds the limit.
    PayloadTooLarge,
    /// 413: storing the file would pass the user's storage quota.
    StorageQuotaExceeded,
    /// 415: the file is not an accepted image or video.
    UnsupportedMediaType,
    /// 422: the request failed validation.
    ValidationFailed,
    /// 422: the platform rejected the pasted credentials.
    CredentialsInvalid,
    /// 422: the idempotency key was reused with a different request.
    IdempotencyKeyReused,
    /// 422: `return_to` is not one of the deployment's allowed return URLs.
    ReturnToInvalid,
    /// 422: the push endpoint or one of its keys is malformed.
    PushEndpointInvalid,
    /// 428: the request needs an `If-Match` header.
    PreconditionRequired,
    /// 429: too many requests.
    RateLimited,
    /// 500: unexpected failure.
    Internal,
    /// 503: a dependency is not ready.
    Unavailable,
    /// 503: the platform is not configured here, or its API is down.
    PlatformUnavailable,
}

impl ErrorCode {
    /// Every code, in declaration order.
    pub const ALL: [Self; 36] = [
        Self::Unauthenticated,
        Self::AccountPending,
        Self::AccountDisabled,
        Self::Forbidden,
        Self::OwnerOnly,
        Self::CapabilityRequired,
        Self::DelegationInvalid,
        Self::AccountNotInScope,
        Self::NotFound,
        Self::PushDisabled,
        Self::RequestTimeout,
        Self::UserDeleting,
        Self::LastAdmin,
        Self::IdempotencyInProgress,
        Self::JobActionNotAllowed,
        Self::PlatformDisabled,
        Self::CapabilityUnsupported,
        Self::SendingMethodExists,
        Self::DelegationExists,
        Self::JobActive,
        Self::MediaInUse,
        Self::UploadOffsetMismatch,
        Self::VersionConflict,
        Self::PayloadTooLarge,
        Self::StorageQuotaExceeded,
        Self::UnsupportedMediaType,
        Self::ValidationFailed,
        Self::CredentialsInvalid,
        Self::IdempotencyKeyReused,
        Self::ReturnToInvalid,
        Self::PushEndpointInvalid,
        Self::PreconditionRequired,
        Self::RateLimited,
        Self::Internal,
        Self::Unavailable,
        Self::PlatformUnavailable,
    ];

    /// The `snake_case` value sent in the `code` member.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::AccountPending => "account_pending",
            Self::AccountDisabled => "account_disabled",
            Self::Forbidden => "forbidden",
            Self::OwnerOnly => "owner_only",
            Self::CapabilityRequired => "capability_required",
            Self::DelegationInvalid => "delegation_invalid",
            Self::AccountNotInScope => "account_not_in_scope",
            Self::NotFound => "not_found",
            Self::PushDisabled => "push_disabled",
            Self::RequestTimeout => "request_timeout",
            Self::UserDeleting => "user_deleting",
            Self::LastAdmin => "last_admin",
            Self::IdempotencyInProgress => "idempotency_in_progress",
            Self::JobActionNotAllowed => "job_action_not_allowed",
            Self::PlatformDisabled => "platform_disabled",
            Self::CapabilityUnsupported => "capability_unsupported",
            Self::SendingMethodExists => "sending_method_exists",
            Self::DelegationExists => "delegation_exists",
            Self::JobActive => "job_active",
            Self::MediaInUse => "media_in_use",
            Self::UploadOffsetMismatch => "upload_offset_mismatch",
            Self::VersionConflict => "version_conflict",
            Self::PayloadTooLarge => "payload_too_large",
            Self::StorageQuotaExceeded => "storage_quota_exceeded",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::ValidationFailed => "validation_failed",
            Self::CredentialsInvalid => "credentials_invalid",
            Self::IdempotencyKeyReused => "idempotency_key_reused",
            Self::ReturnToInvalid => "return_to_invalid",
            Self::PushEndpointInvalid => "push_endpoint_invalid",
            Self::PreconditionRequired => "precondition_required",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
            Self::PlatformUnavailable => "platform_unavailable",
        }
    }

    /// The HTTP status the code is sent with.
    #[must_use]
    pub fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::AccountPending
            | Self::AccountDisabled
            | Self::Forbidden
            | Self::OwnerOnly
            | Self::CapabilityRequired
            | Self::DelegationInvalid
            | Self::AccountNotInScope => StatusCode::FORBIDDEN,
            Self::NotFound | Self::PushDisabled => StatusCode::NOT_FOUND,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::UserDeleting
            | Self::LastAdmin
            | Self::IdempotencyInProgress
            | Self::JobActionNotAllowed
            | Self::PlatformDisabled
            | Self::CapabilityUnsupported
            | Self::SendingMethodExists
            | Self::DelegationExists
            | Self::JobActive
            | Self::MediaInUse
            | Self::UploadOffsetMismatch => StatusCode::CONFLICT,
            Self::VersionConflict => StatusCode::PRECONDITION_FAILED,
            Self::PayloadTooLarge | Self::StorageQuotaExceeded => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ValidationFailed
            | Self::CredentialsInvalid
            | Self::IdempotencyKeyReused
            | Self::ReturnToInvalid
            | Self::PushEndpointInvalid => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable | Self::PlatformUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// The short human-readable `title` of the problem.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Unauthenticated => "Authentication required",
            Self::AccountPending => "Account pending approval",
            Self::AccountDisabled => "Account disabled",
            Self::Forbidden => "Forbidden",
            Self::OwnerOnly => "Only the workspace owner may do this",
            Self::CapabilityRequired => "Delegation does not allow this",
            Self::DelegationInvalid => "Delegation is not valid",
            Self::AccountNotInScope => "The action reaches accounts outside your delegation",
            Self::NotFound => "Not found",
            Self::PushDisabled => "Push notifications are off",
            Self::RequestTimeout => "Request timed out",
            Self::UserDeleting => "User is being deleted",
            Self::LastAdmin => "Last active admin",
            Self::IdempotencyInProgress => "Request with this key is in progress",
            Self::JobActionNotAllowed => "Job action not allowed",
            Self::PlatformDisabled => "Platform disabled",
            Self::CapabilityUnsupported => "Not supported by this platform",
            Self::SendingMethodExists => "Sending method already exists",
            Self::DelegationExists => "A delegation to this user already exists",
            Self::JobActive => "A post is being published",
            Self::MediaInUse => "Media is in use",
            Self::UploadOffsetMismatch => "Upload offset mismatch",
            Self::VersionConflict => "Version conflict",
            Self::PayloadTooLarge => "Payload too large",
            Self::StorageQuotaExceeded => "Storage quota exceeded",
            Self::UnsupportedMediaType => "Unsupported media type",
            Self::ValidationFailed => "Validation failed",
            Self::CredentialsInvalid => "Credentials invalid",
            Self::IdempotencyKeyReused => "Idempotency key reused",
            Self::ReturnToInvalid => "Return URL not allowed",
            Self::PushEndpointInvalid => "Push endpoint invalid",
            Self::PreconditionRequired => "Precondition required",
            Self::RateLimited => "Too many requests",
            Self::Internal => "Internal error",
            Self::Unavailable => "Service unavailable",
            Self::PlatformUnavailable => "Platform unavailable",
        }
    }
}

/// The RFC 9457 `application/problem+json` body every error response carries.
#[derive(Debug, ToSchema)]
pub struct ProblemDetails {
    /// Always `about:blank`; `code` is the discriminator.
    pub r#type: String,
    /// Short summary of the problem type.
    pub title: String,
    /// The HTTP status code.
    pub status: u16,
    /// Machine-readable code.
    pub code: ErrorCode,
    /// The `X-Request-Id` of the failed request, for support.
    pub request_id: Option<String>,
    /// Explanation specific to this occurrence.
    pub detail: Option<String>,
    /// `validation_failed` on account routes: every rule the input broke, by field.
    #[schema(value_type = Option<Vec<Object>>)]
    pub violations: Option<Vec<serde_json::Value>>,
    /// `credentials_invalid`: the credential field at fault, when known. `push_endpoint_invalid`:
    /// `endpoint`, `p256dh`, or `auth`.
    pub field: Option<String>,
    /// `credentials_invalid`: scopes the credentials lack, when the platform says.
    pub missing_scopes: Option<Vec<String>>,
}

/// The data behind a problem response.
#[derive(Debug, Clone)]
pub struct Problem {
    /// Machine-readable code.
    pub code: ErrorCode,
    /// Explanation specific to this occurrence.
    pub detail: Option<String>,
    /// How long the client should wait, sent as `Retry-After`.
    pub retry_after: Option<Duration>,
    /// `unauthenticated` because the request carried no usable bearer token at all, as
    /// opposed to one that failed verification. RFC 6750 §3.1: the challenge then carries no
    /// `error` code.
    pub token_missing: bool,
    /// Members added to the body next to the standard ones (`violations`, `field`, ...).
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

// Clone: axum's closure `Handler` impl requires `Clone`, so tests (and any handler that
// returns a captured error) need it; `Problem` is already `Clone`.
/// An error a handler returns; rendered as problem+json.
#[derive(Debug, Clone)]
pub struct ApiError(pub Problem);

impl ApiError {
    /// An error with `code` and no detail.
    #[must_use]
    pub fn new(code: ErrorCode) -> Self {
        Self(Problem {
            code,
            detail: None,
            retry_after: None,
            token_missing: false,
            extensions: serde_json::Map::new(),
        })
    }

    /// Adds a member to the problem body.
    #[must_use]
    pub fn with_extension(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.0.extensions.insert(key.to_owned(), value.into());
        self
    }

    /// `unauthenticated` for a request with no usable bearer token (no `Authorization`
    /// header, another scheme, or an empty token): `WWW-Authenticate: Bearer`, no error code.
    #[must_use]
    pub fn missing_token() -> Self {
        let mut err = Self::new(ErrorCode::Unauthenticated);
        err.0.token_missing = true;
        err
    }

    /// Adds a human-readable explanation to the problem.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.0.detail = Some(detail.into());
        self
    }

    /// Sets the `Retry-After` the response carries.
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

impl From<postit_data::ScopeError> for ApiError {
    fn from(err: postit_data::ScopeError) -> Self {
        match err {
            postit_data::ScopeError::OwnerOnly => Self::new(ErrorCode::OwnerOnly),
            postit_data::ScopeError::InsufficientAccess => Self::new(ErrorCode::CapabilityRequired),
        }
    }
}

impl From<DataError> for ApiError {
    fn from(err: DataError) -> Self {
        match err {
            // A row outside the account limit stays hidden; an object that is visible but
            // touches accounts outside the limit is refused out loud (the whole-object rule).
            DataError::NotFound => Self::new(ErrorCode::NotFound),
            DataError::AccountNotInScope => Self::new(ErrorCode::AccountNotInScope),
            DataError::Scope(scope) => scope.into(),
            DataError::SendingMethodExists => Self::new(ErrorCode::SendingMethodExists),
            other => Self::internal(&other),
        }
    }
}

impl From<postit_oauth::AccountError> for ApiError {
    fn from(err: postit_oauth::AccountError) -> Self {
        use postit_oauth::AccountError as E;
        match err {
            E::PlatformNotFound | E::NotFound => Self::new(ErrorCode::NotFound),
            E::PlatformDisabled => Self::new(ErrorCode::PlatformDisabled),
            E::PlatformUnavailable => Self::new(ErrorCode::PlatformUnavailable),
            E::ConnectMethodUnsupported => Self::new(ErrorCode::CapabilityUnsupported)
                .with_detail("this platform connects another way"),
            E::Validation(violations) => Self::new(ErrorCode::ValidationFailed).with_extension(
                "violations",
                serde_json::to_value(violations).unwrap_or_default(),
            ),
            E::CredentialsInvalid {
                field,
                missing_scopes,
            } => {
                let mut error = Self::new(ErrorCode::CredentialsInvalid);
                if let Some(field) = field {
                    error = error.with_extension("field", field);
                }
                if !missing_scopes.is_empty() {
                    error = error.with_extension("missing_scopes", missing_scopes);
                }
                error
            }
            E::RateLimited { retry_after } => {
                let error = Self::new(ErrorCode::RateLimited);
                match retry_after {
                    Some(wait) => error.with_retry_after(wait),
                    None => error,
                }
            }
            E::AccountDisconnected => {
                Self::new(ErrorCode::ValidationFailed).with_detail("the account is disconnected")
            }
            E::JobActive => Self::new(ErrorCode::JobActive),
            E::SendingMethodExists => Self::new(ErrorCode::SendingMethodExists),
            E::ReturnToInvalid => Self::new(ErrorCode::ReturnToInvalid),
            // The platform learns nothing about why its event was refused.
            E::EventRejected => Self::new(ErrorCode::Unauthenticated),
            E::EventMalformed => {
                Self::new(ErrorCode::ValidationFailed).with_detail("the event could not be read")
            }
            E::CsvInvalid(message) => Self::new(ErrorCode::ValidationFailed).with_detail(message),
            E::Scope(scope) => scope.into(),
            E::Data(data) => data.into(),
            E::Vault(other) => Self::internal(&other),
        }
    }
}

impl From<postit_media::MediaServiceError> for ApiError {
    fn from(err: postit_media::MediaServiceError) -> Self {
        use postit_media::MediaServiceError as E;
        match err {
            E::NotFound | E::NotPending => Self::new(ErrorCode::NotFound),
            E::Scope(scope) => scope.into(),
            E::UnsupportedMediaType => Self::new(ErrorCode::UnsupportedMediaType),
            E::PayloadTooLarge { max } => {
                Self::new(ErrorCode::PayloadTooLarge).with_extension("max_bytes", max)
            }
            E::StorageQuotaExceeded { quota, used } => Self::new(ErrorCode::StorageQuotaExceeded)
                .with_extension("quota_bytes", quota)
                .with_extension("used_bytes", used),
            E::UploadOffsetMismatch { offset } => {
                Self::new(ErrorCode::UploadOffsetMismatch).with_extension("offset", offset)
            }
            E::InUse => Self::new(ErrorCode::MediaInUse),
            E::Invalid { field } => {
                Self::new(ErrorCode::ValidationFailed).with_extension("field", field)
            }
            E::BodyUnreadable => Self::new(ErrorCode::ValidationFailed)
                .with_detail("the request body could not be read"),
            E::BodyTimedOut => Self::new(ErrorCode::RequestTimeout),
            E::Data(data) => data.into(),
            E::Io(io) => Self::internal(&io),
        }
    }
}

impl From<postit_notify::PushEndpointError> for ApiError {
    fn from(err: postit_notify::PushEndpointError) -> Self {
        use postit_notify::PushEndpointError as E;
        match err {
            E::Disabled => Self::new(ErrorCode::PushDisabled),
            E::Invalid {
                field: "device_label",
            } => Self::new(ErrorCode::ValidationFailed).with_extension("field", "device_label"),
            // The field name only: the endpoint and keys are secrets.
            E::Invalid { field } => {
                Self::new(ErrorCode::PushEndpointInvalid).with_extension("field", field)
            }
            E::NotFound => Self::new(ErrorCode::NotFound),
            E::Data(data) => data.into(),
            E::Vault(other) => Self::internal(&other),
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

impl From<DelegationError> for ApiError {
    fn from(err: DelegationError) -> Self {
        match err {
            DelegationError::NotFound => Self::new(ErrorCode::NotFound),
            DelegationError::Exists => Self::new(ErrorCode::DelegationExists),
            DelegationError::Invalid { field } => {
                Self::new(ErrorCode::ValidationFailed).with_extension("field", field)
            }
            DelegationError::Scope(scope) => scope.into(),
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
            extensions: serde_json::Map::new(),
        })
    });
    let Some(problem) = problem else {
        return response;
    };

    let mut body = serde_json::json!({
        "type": "about:blank",
        "title": problem.code.title(),
        "status": problem.code.status().as_u16(),
        "code": problem.code.as_str(),
        "request_id": request_id,
        "detail": problem.detail,
    });
    if let Some(object) = body.as_object_mut() {
        for (key, value) in &problem.extensions {
            object.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
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

    #[test]
    fn schema_codes_match_the_wire_codes() {
        let schema = serde_json::to_value(<ErrorCode as utoipa::PartialSchema>::schema())
            .unwrap_or_default();
        let listed: Vec<&str> = schema["enum"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let wire: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(listed, wire);
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
    async fn whole_object_refusal_is_403_and_a_hidden_row_stays_404() {
        let (status, _, body) = render(ApiError::from(DataError::AccountNotInScope)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "account_not_in_scope");
        let (status, _, body) = render(ApiError::from(DataError::NotFound)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test]
    async fn delegation_errors_map_to_their_codes() {
        let cases = [
            (
                DelegationError::NotFound,
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                DelegationError::Exists,
                StatusCode::CONFLICT,
                "delegation_exists",
            ),
            (
                DelegationError::Invalid {
                    field: "expires_at",
                },
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
            ),
            (
                DelegationError::Scope(postit_data::ScopeError::OwnerOnly),
                StatusCode::FORBIDDEN,
                "owner_only",
            ),
            (
                DelegationError::Scope(postit_data::ScopeError::InsufficientAccess),
                StatusCode::FORBIDDEN,
                "capability_required",
            ),
            (
                DelegationError::Data(DataError::Conflict("x".into())),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
        ];
        for (err, status, code) in cases {
            let (got, _, body) = render(err.into()).await;
            assert_eq!(got, status, "{code}");
            assert_eq!(body["code"], code);
        }
        let (_, _, body) = render(
            DelegationError::Invalid {
                field: "expires_at",
            }
            .into(),
        )
        .await;
        assert_eq!(body["field"], "expires_at");
    }

    #[tokio::test]
    async fn oauth_errors_map_to_their_codes() {
        use postit_oauth::AccountError;
        let (status, _, body) = render(AccountError::ReturnToInvalid.into()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "return_to_invalid");
        let (status, headers, body) = render(AccountError::EventRejected.into()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["detail"].is_null(), "{body}");
        assert!(headers.contains_key(header::WWW_AUTHENTICATE));
        let (status, _, body) = render(AccountError::EventMalformed.into()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "validation_failed");
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
