//! The signed-in user.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::NotificationKind;
use postit_data::preferences::{
    ChannelSwitch, NotificationSwitches, PreferencesUpdate, UserPreferences, UserPreferencesRepo,
};
use postit_data::users::{UserRecord, UsersRepo};
use postit_i18n::Locale;

use crate::dto::{
    DeleteMeRequest, KindSwitchDto, LocaleDto, MeDto, PreferencesDto, UpdatePreferences, UserDto,
};
use crate::dto_delegations::WorkspaceDto;
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{ActiveUser, Auth, RequestAudit};
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
    responses((status = 200, body = MeDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled` (disabled or deleting account) or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
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
/// Returns 403 while pending (or for a disabled or already-deleting account, which `Auth`
/// rejects first), 422 when the display name does not match, 409 for the last active admin.
#[utoipa::path(delete, path = "/api/v1/me", tag = "me", security(("oidc" = [])),
    request_body = DeleteMeRequest,
    responses((status = 202, description = "Deletion started"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`last_admin`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_me(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    RequestAudit(audit): RequestAudit,
    ApiJson(body): ApiJson<DeleteMeRequest>,
) -> Result<StatusCode, ApiError> {
    let user = load(&state, principal.user_id).await?;
    if body.display_name != user.display_name {
        return Err(ApiError::new(ErrorCode::ValidationFailed)
            .with_detail("display_name does not match your current display name"));
    }
    state.admin.delete_self(principal.user_id, audit).await?;
    state.auth.invalidate_user(principal.user_id);
    Ok(StatusCode::ACCEPTED)
}

/// The caller's preferences. Reachable while the account is still pending approval.
///
/// # Errors
///
/// Returns 401 without a valid token, 403 for a disabled account.
#[utoipa::path(get, path = "/api/v1/me/preferences", tag = "me", security(("oidc" = [])),
    responses((status = 200, body = PreferencesDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled` (disabled or deleting account) or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_preferences(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<PreferencesDto>, ApiError> {
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    // Every user gets a row when provisioned; a missing one reads as the column defaults.
    let prefs = match UserPreferencesRepo::get(&mut conn, principal.user_id).await? {
        Some(prefs) => prefs,
        None => {
            UserPreferencesRepo::upsert(&mut conn, principal.user_id, &PreferencesUpdate::default())
                .await?
        }
    };
    Ok(Json(preferences_dto(&prefs)))
}

fn preferences_dto(prefs: &UserPreferences) -> PreferencesDto {
    let locale = Locale::from_tag(&prefs.locale).unwrap_or_default();
    let switches = &prefs.notification_switches;
    PreferencesDto {
        locale: LocaleDto::from(locale),
        timezone: prefs.timezone.clone(),
        email_notifications: prefs.email_notifications,
        comment_digest: prefs.comment_digest,
        switches: NotificationKind::ALL
            .iter()
            .map(|&kind| KindSwitchDto {
                kind,
                push: switches.push(kind),
                email: kind.email_allowed().then(|| switches.email(kind)),
            })
            .collect(),
    }
}

fn validation(detail: &str) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed).with_detail(detail)
}

fn to_update(body: UpdatePreferences) -> Result<PreferencesUpdate, ApiError> {
    if let Some(tz) = &body.timezone
        && tz.parse::<chrono_tz::Tz>().is_err()
    {
        return Err(validation("timezone must be an IANA time zone name"));
    }
    let switches = match body.switches {
        None => None,
        Some(list) => {
            let mut map = BTreeMap::new();
            for item in list {
                if item.email.is_some() && !item.kind.email_allowed() {
                    return Err(validation(
                        "switches: email can only be set for kinds that are emailed",
                    ));
                }
                let entry: &mut ChannelSwitch = map.entry(item.kind).or_default();
                entry.push = item.push.or(entry.push);
                entry.email = item.email.or(entry.email);
            }
            Some(NotificationSwitches(map))
        }
    };
    Ok(PreferencesUpdate {
        locale: body.locale.map(|l| Locale::from(l).tag().to_owned()),
        timezone: body.timezone,
        email_notifications: body.email_notifications,
        switches,
        comment_digest: body.comment_digest,
    })
}

/// Changes the caller's preferences. Reachable while the account is still pending approval.
///
/// # Errors
///
/// Returns 401 without a valid token, 403 for a disabled account, 422 for an invalid time zone or
/// an email switch on a kind that is never emailed.
#[utoipa::path(put, path = "/api/v1/me/preferences", tag = "me", security(("oidc" = [])),
    request_body = UpdatePreferences,
    responses((status = 200, body = PreferencesDto), (status = 422, description = "`validation_failed` (invalid time zone, or an email switch on a kind that is never emailed)", body = ProblemDetails, content_type = "application/problem+json"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled` (disabled or deleting account) or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn put_preferences(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiJson(body): ApiJson<UpdatePreferences>,
) -> Result<Json<PreferencesDto>, ApiError> {
    let update = to_update(body)?;
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let prefs = UserPreferencesRepo::upsert(&mut conn, principal.user_id, &update).await?;
    Ok(Json(preferences_dto(&prefs)))
}

/// The workspaces the caller can act in: their own first, then each one a live grant opens
/// whose owner is active, newest grant first. Send a workspace's `owner.id` in
/// `X-Postit-Act-As` on workspace routes to act there. Does not accept `X-Postit-Act-As`
/// itself (`owner_only`).
///
/// # Errors
///
/// Returns 401 without a valid token, 403 while pending or disabled.
#[utoipa::path(get, path = "/api/v1/me/workspaces", tag = "me", security(("oidc" = [])),
    responses((status = 200, body = Vec<WorkspaceDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn workspaces(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
) -> Result<Json<Vec<WorkspaceDto>>, ApiError> {
    let list = state.delegations.workspaces(principal.user_id).await?;
    Ok(Json(list.into_iter().map(WorkspaceDto::from).collect()))
}
