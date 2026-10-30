//! The caller's own notifications: list, mark read, and the unread count the bell polls.
//! Every route is self only: `X-Postit-Act-As` is refused (`owner_only`) and a pending user is
//! refused (`account_pending`). A delegate's list holds the rows recorded for them, including
//! those about workspaces they act in; `owner_id` tells which.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::NotificationId;
use postit_data::notifications::{NotificationRepo, ReadSelection, UnreadState};
use utoipa::IntoParams;

use crate::dto_notifications::{
    MarkReadRequest, MarkReadResponse, NotificationDto, UnreadCountDto,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::ActiveUser;
use crate::json::{ApiJson, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// Most IDs one read request may name.
const MAX_READ_IDS: usize = 200;

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, serde::Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct NotificationListQuery {
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// The caller's notifications, newest first.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/notifications", operation_id = "list_notifications", tag = "notifications", security(("oidc" = [])),
    params(NotificationListQuery),
    responses((status = 200, body = Page<NotificationDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiQuery(q): ApiQuery<NotificationListQuery>,
) -> Result<Json<Page<NotificationDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let page = NotificationRepo::list(&mut conn, principal.user_id, pagination).await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(NotificationDto::from).collect(),
        page.total,
        &pagination,
    )))
}

fn selection(body: MarkReadRequest) -> Result<ReadSelection, ApiError> {
    let invalid = |detail: &str| ApiError::new(ErrorCode::ValidationFailed).with_detail(detail);
    match (body.ids, body.all) {
        (Some(ids), None | Some(false)) => {
            if ids.is_empty() || ids.len() > MAX_READ_IDS {
                return Err(invalid(&format!("ids must hold 1 to {MAX_READ_IDS} IDs")));
            }
            Ok(ReadSelection::Ids(
                ids.into_iter().map(NotificationId::from_uuid).collect(),
            ))
        }
        (None, Some(true)) => Ok(ReadSelection::All),
        _ => Err(invalid("send either ids or all: true")),
    }
}

/// Marks the caller's notifications read: the named ones, or all of them. IDs that are not the
/// caller's are ignored.
///
/// # Errors
///
/// 422 unless the body names exactly one of `ids` (1 to 200) or `all: true`.
#[utoipa::path(post, path = "/api/v1/notifications/read", operation_id = "mark_notifications_read", tag = "notifications", security(("oidc" = [])),
    request_body = MarkReadRequest,
    responses((status = 200, body = MarkReadResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn mark_read(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiJson(body): ApiJson<MarkReadRequest>,
) -> Result<Json<MarkReadResponse>, ApiError> {
    let selection = selection(body)?;
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let updated =
        NotificationRepo::mark_read(&mut conn, principal.user_id, &selection, state.clock.now())
            .await?;
    Ok(Json(MarkReadResponse { updated }))
}

/// The weak entity tag of an unread state: `W/"<count>-<newest id or 0>"`.
fn unread_etag(state: &UnreadState) -> String {
    let newest = state
        .newest
        .map_or_else(|| "0".to_owned(), |id| id.as_uuid().to_string());
    format!("W/\"{}-{newest}\"", state.count)
}

/// Whether `If-None-Match` names `etag`, by weak comparison (RFC 9110): `*`, or any listed tag
/// equal once the `W/` prefix is dropped.
fn none_match(headers: &HeaderMap, etag: &str) -> bool {
    let strip = |tag: &str| tag.trim().trim_start_matches("W/").to_owned();
    let current = strip(etag);
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|tag| tag.trim() == "*" || strip(tag) == current)
}

/// How many notifications the caller has not read. Answers 304 with no body when
/// `If-None-Match` holds the current `ETag`, so the bell can poll cheaply.
///
/// # Errors
///
/// None beyond authentication.
#[utoipa::path(get, path = "/api/v1/notifications/unread-count", operation_id = "unread_notification_count", tag = "notifications", security(("oidc" = [])),
    responses((status = 200, body = UnreadCountDto, headers(("ETag" = String, description = "Weak tag `W/\"<count>-<newest unread id or 0>\"`"))), (status = 304, description = "`If-None-Match` matches the current `ETag`"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn unread_count(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let unread = NotificationRepo::unread_state(&mut conn, principal.user_id).await?;
    let etag = unread_etag(&unread);
    let tag = HeaderValue::from_str(&etag).map_err(|e| ApiError::internal(&e))?;
    let mut response = if none_match(&headers, &etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(UnreadCountDto {
            count: unread.count,
        })
        .into_response()
    };
    response.headers_mut().insert(header::ETAG, tag);
    Ok(response)
}
