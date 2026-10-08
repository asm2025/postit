use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::UserId;
use postit_data::users::{UserListFilter, UserRole, UserSort, UserStatus, UsersRepo};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto::{PatchUserRequest, StatusDto, UserDto};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::RequireAdmin;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// Flat on purpose: `#[serde(flatten)]` inside a `Query` makes `serde_urlencoded` buffer
/// values as strings, and `Option<u64>` then fails to parse (`?page_size=2` would be 422).
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct UserListQuery {
    /// `pending`, `active`, `disabled`, or `deleting`.
    pub status: Option<String>,
    /// `admin` or `member`.
    pub role: Option<String>,
    /// `created_at` (oldest first) or `-created_at` (newest first, the default).
    pub sort: Option<String>,
    /// Case-insensitive substring of display name or email.
    pub search: Option<String>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Lists users, filtered by status, role, and a name/email search, ordered by `sort`.
///
/// # Errors
///
/// Returns 403 for non-admins and 422 for invalid filters or paging.
#[utoipa::path(get, path = "/api/v1/users", operation_id = "list_users", tag = "users", security(("oidc" = [])),
    params(UserListQuery), responses((status = 200, body = Page<UserDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<UserListQuery>,
) -> Result<Json<Page<UserDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let status = q
        .status
        .as_deref()
        .map(str::parse::<UserStatus>)
        .transpose()
        .map_err(|_| ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown status"))?;
    let role = q
        .role
        .as_deref()
        .map(str::parse::<UserRole>)
        .transpose()
        .map_err(|_| ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown role"))?;
    let sort = match q.sort.as_deref() {
        None | Some("-created_at") => UserSort::CreatedAtDesc,
        Some("created_at") => UserSort::CreatedAtAsc,
        Some(_) => {
            return Err(ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown sort"));
        }
    };
    let search = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let filter = UserListFilter {
        status,
        role,
        search,
        sort,
    };
    let result = UsersRepo::list(&mut conn, &filter, pagination).await?;
    let data = result.data.iter().map(UserDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}

/// One user by id.
///
/// # Errors
///
/// Returns 403 for non-admins and 404 for an unknown id.
#[utoipa::path(get, path = "/api/v1/users/{id}", operation_id = "get_user", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), responses((status = 200, body = UserDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<UserDto>, ApiError> {
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let user = UsersRepo::find_by_id(&mut conn, UserId::from(id))
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    Ok(Json(UserDto::from(&user)))
}

/// Changes one user's status or role (exactly one field per request).
///
/// # Errors
///
/// Returns 403 for non-admins, 404 for an unknown id, 409 for a user being deleted or the
/// last admin, and 422 for a malformed or disallowed change.
#[utoipa::path(patch, path = "/api/v1/users/{id}", operation_id = "update_user", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), request_body = PatchUserRequest,
    responses((status = 200, body = UserDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`user_deleting` or `last_admin`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn patch(
    State(state): State<AppState>,
    RequireAdmin(actor): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PatchUserRequest>,
) -> Result<Json<UserDto>, ApiError> {
    let target = UserId::from(id);
    let record = match (body.status, body.role) {
        (Some(status), None) => {
            let mut conn = state
                .pool
                .acquire()
                .await
                .map_err(|e| ApiError::internal(&e))?;
            let current = UsersRepo::find_by_id(&mut conn, target)
                .await?
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
            drop(conn);
            match (current.status, status) {
                // Spec: `pending` and `deleting` are never settable targets, whatever the
                // current status, so this arm comes before the `user_deleting` one.
                (_, StatusDto::Pending | StatusDto::Deleting) => {
                    return Err(ApiError::new(ErrorCode::ValidationFailed)
                        .with_detail("status can only be set to `active` or `disabled`"));
                }
                (UserStatus::Deleting, _) => return Err(ApiError::new(ErrorCode::UserDeleting)),
                (UserStatus::Pending, StatusDto::Active) => {
                    state.admin.approve(actor.user_id, target).await?
                }
                (UserStatus::Disabled, StatusDto::Active) => {
                    state.admin.enable(actor.user_id, target).await?
                }
                (_, StatusDto::Disabled) => state.admin.disable(actor.user_id, target).await?,
                (from, _) => {
                    return Err(
                        ApiError::new(ErrorCode::ValidationFailed).with_detail(format!(
                            "status transition not allowed: {} -> {}",
                            from.as_str(),
                            UserStatus::from(status).as_str()
                        )),
                    );
                }
            }
        }
        (None, Some(role)) => {
            state
                .admin
                .change_role(actor.user_id, target, role.into())
                .await?
        }
        _ => {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail("send exactly one of `status` or `role`"));
        }
    };
    // This process sees the change on the next request; pg_notify covers the others.
    state.auth.invalidate_user(target);
    Ok(Json(UserDto::from(&record)))
}

/// Starts deleting a user's account.
///
/// # Errors
///
/// Returns 403 for non-admins or self-deletion, 404 for an unknown id, 409 for the last
/// admin or a user already being deleted.
#[utoipa::path(delete, path = "/api/v1/users/{id}", operation_id = "delete_user", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 202, description = "Deletion started"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`user_deleting` or `last_admin`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete(
    State(state): State<AppState>,
    RequireAdmin(actor): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .admin
        .delete_user(actor.user_id, UserId::from(id))
        .await?;
    state.auth.invalidate_user(UserId::from(id));
    Ok(StatusCode::ACCEPTED)
}
