use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_data::users::{UserRecord, UsersRepo};

use crate::dto::{DeleteMeRequest, MeDto, UserDto};
use crate::error::{ApiError, ErrorCode};
use crate::extract::{ActiveUser, Auth};
use crate::json::ApiJson;
use crate::state::AppState;

async fn load(state: &AppState, id: postit_core::UserId) -> Result<UserRecord, ApiError> {
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    UsersRepo::find_by_id(&mut conn, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))
}

/// The caller's own profile. Reachable while the account is still pending approval.
///
/// # Errors
///
/// Returns 401 without a valid token, 403 for a disabled account.
#[utoipa::path(get, path = "/api/v1/me", tag = "me", security(("oidc" = [])),
    responses((status = 200, body = MeDto), (status = 401), (status = 403)))]
pub async fn get_me(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<MeDto>, ApiError> {
    let user = load(&state, principal.user_id).await?;
    Ok(Json(MeDto {
        user: UserDto::from(&user),
        account_url: state.settings.account_url.as_ref().map(ToString::to_string),
    }))
}

/// Starts deleting the caller's own account.
///
/// # Errors
///
/// Returns 403 while pending, 422 when the display name does not match, 409 for the last
/// active admin or an account already being deleted.
#[utoipa::path(delete, path = "/api/v1/me", tag = "me", security(("oidc" = [])),
    request_body = DeleteMeRequest,
    responses((status = 202, description = "Deletion started"), (status = 409), (status = 422)))]
pub async fn delete_me(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiJson(body): ApiJson<DeleteMeRequest>,
) -> Result<StatusCode, ApiError> {
    let user = load(&state, principal.user_id).await?;
    if body.display_name != user.display_name {
        return Err(ApiError::new(ErrorCode::ValidationFailed)
            .with_detail("display_name does not match your current display name"));
    }
    state.admin.delete_self(principal.user_id).await?;
    state.auth.invalidate_user(principal.user_id);
    Ok(StatusCode::ACCEPTED)
}
