use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::UserId;
use postit_data::users::{UserStatus, UsersRepo};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto::{PatchUserRequest, StatusDto, UserDto};
use crate::error::{ApiError, ErrorCode};
use crate::extract::RequireAdmin;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// Flat on purpose: `#[serde(flatten)]` inside a `Query` makes `serde_urlencoded` buffer
/// values as strings, and `Option<u64>` then fails to parse (`?page_size=2` would be 422).
#[derive(Debug, Deserialize, IntoParams)]
pub struct UserListQuery {
    /// `pending`, `active`, `disabled`, or `deleting`.
    pub status: Option<String>,
    /// Case-insensitive substring of display name or email.
    pub search: Option<String>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Lists users, optionally filtered by status and a name/email search.
///
/// # Errors
///
/// Returns 403 for non-admins and 422 for invalid filters or paging.
#[utoipa::path(get, path = "/api/v1/users", tag = "users", security(("oidc" = [])),
    params(UserListQuery), responses((status = 200, body = Page<UserDto>), (status = 403), (status = 422)))]
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
    let search = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let result = UsersRepo::list(&mut conn, status, search, pagination).await?;
    let data = result.data.iter().map(UserDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}

/// One user by id.
///
/// # Errors
///
/// Returns 403 for non-admins and 404 for an unknown id.
#[utoipa::path(get, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), responses((status = 200, body = UserDto), (status = 404)))]
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
#[utoipa::path(patch, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), request_body = PatchUserRequest,
    responses((status = 200, body = UserDto), (status = 404), (status = 409), (status = 422)))]
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
#[utoipa::path(delete, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 202, description = "Deletion started"), (status = 403), (status = 404), (status = 409)))]
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
