//! The VAPID public key and the caller's own push devices. Every route is self only:
//! `X-Postit-Act-As` is refused (`owner_only`) and a pending user is refused
//! (`account_pending`). The endpoint and keys never appear in a response or a problem body.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::PushEndpointId;
use postit_notify::NewDevice;
use uuid::Uuid;

use crate::dto_push::{PushEndpointCreated, RegisterPushEndpointRequest, VapidKeyDto};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::ActiveUser;
use crate::json::{ApiJson, ApiPath};
use crate::state::AppState;

/// The deployment's VAPID public key, which browsers subscribe with.
///
/// # Errors
///
/// 404 `push_disabled` when push is off.
#[utoipa::path(get, path = "/api/v1/push/vapid-public-key", operation_id = "get_vapid_public_key", tag = "push", security(("oidc" = [])),
    responses((status = 200, body = VapidKeyDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`push_disabled`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn vapid_public_key(
    State(state): State<AppState>,
    ActiveUser(_principal): ActiveUser,
) -> Result<Json<VapidKeyDto>, ApiError> {
    state
        .push
        .vapid_public_key()
        .map(|key| Json(VapidKeyDto { key }))
        .ok_or_else(|| ApiError::new(ErrorCode::PushDisabled))
}

/// Registers one of the caller's devices for push notifications. The same endpoint again
/// replaces its keys, platform, and label, and answers with the same ID.
///
/// # Errors
///
/// 404 `push_disabled` when push is off; 422 `push_endpoint_invalid` (with `field`) for a
/// malformed endpoint or key, `validation_failed` for a label over 100 characters.
#[utoipa::path(post, path = "/api/v1/me/push-endpoints", operation_id = "register_push_endpoint", tag = "push", security(("oidc" = [])),
    request_body = RegisterPushEndpointRequest,
    responses((status = 201, body = PushEndpointCreated), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`push_disabled`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`push_endpoint_invalid`; `field` names `endpoint` (not `https`, or `http` where private endpoints are not allowed), `p256dh` (not a 65-byte P-256 point), or `auth` (not 16 bytes). `validation_failed` for the label or a malformed body", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn register(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiJson(body): ApiJson<RegisterPushEndpointRequest>,
) -> Result<(StatusCode, Json<PushEndpointCreated>), ApiError> {
    let id = state
        .push
        .register(
            principal.user_id,
            NewDevice {
                endpoint: body.endpoint,
                p256dh: body.p256dh,
                auth: body.auth,
                platform: body.platform.into(),
                device_label: body.device_label,
            },
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(PushEndpointCreated { id: id.as_uuid() }),
    ))
}

/// Removes one of the caller's devices. Works with push off.
///
/// # Errors
///
/// 404 `not_found` when the caller has no such device.
#[utoipa::path(delete, path = "/api/v1/me/push-endpoints/{id}", operation_id = "delete_push_endpoint", tag = "push", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Device ID")),
    responses((status = 204, description = "Removed"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .push
        .delete(principal.user_id, PushEndpointId::from(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
