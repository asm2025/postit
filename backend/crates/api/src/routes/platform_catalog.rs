//! The platform catalog for signed-in users: which platforms can be connected here, and their
//! icons. The bearer-free routes a platform calls live in [`super::platforms`].

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::{AgentDescriptor, PlatformId};

use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::Scope;
use crate::json::ApiPath;
use crate::preconditions::{content_etag, if_none_match};
use crate::responses::CommonProblems;
use crate::state::AppState;

/// The platforms accounts can be connected to here: compiled into this build, configured, and
/// switched on, in the order the connect screen shows them. The icon is left out; fetch it from
/// `GET /platforms/{id}/icon`. Any signed-in user may read it, and it is the same through
/// `X-Postit-Act-As`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/platforms", operation_id = "list_platforms", tag = "platforms", security(("oidc" = [])),
    responses((status = 200, body = Vec<AgentDescriptor>), CommonProblems))]
pub async fn list(
    State(state): State<AppState>,
    Scope(_scope): Scope,
) -> Result<Json<Vec<AgentDescriptor>>, ApiError> {
    Ok(Json(state.accounts.available_platforms().await?))
}

/// The platform's icon as a PNG, cached for a day and tagged with a hash of its bytes. A
/// platform that `GET /platforms` does not list answers 404.
///
/// # Errors
///
/// 404 for a platform that is unknown, not compiled in, not configured, or switched off.
#[utoipa::path(get, path = "/api/v1/platforms/{id}/icon", operation_id = "get_platform_icon", tag = "platforms", security(("oidc" = [])),
    params(("id" = String, Path, description = "Platform ID")),
    responses(
        (status = 200, description = "The icon", content_type = "image/png", body = Vec<u8>, headers(("ETag" = String, description = "Strong tag over the icon bytes"), ("Cache-Control" = String, description = "`public, max-age=86400`"))),
        (status = 304, description = "`If-None-Match` matched"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn icon(
    State(state): State<AppState>,
    Scope(_scope): Scope,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let id = PlatformId::new(&id).map_err(|_| ApiError::new(ErrorCode::NotFound))?;
    let listed = state.accounts.available_platforms().await?;
    let descriptor = listed
        .iter()
        .find(|d| d.platform == id)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let tag = content_etag(descriptor.icon_png);
    let cache = HeaderValue::from_static("public, max-age=86400");
    if if_none_match(&headers, &tag) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [(header::ETAG, tag), (header::CACHE_CONTROL, cache)],
        )
            .into_response());
    }
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
            (header::ETAG, tag),
            (header::CACHE_CONTROL, cache),
        ],
        descriptor.icon_png,
    )
        .into_response())
}
