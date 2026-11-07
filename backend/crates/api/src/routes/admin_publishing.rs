//! Admin routes for the publishing deployment: platforms, the live switch, per-user limits,
//! and vault rotation. Each change writes its audit event in the same transaction.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::{AuditEventId, PlatformId, UserId};
use postit_data::DataError;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::limits::{UserLimits, UserLimitsRepo};
use postit_data::platforms::PlatformRepo;
use postit_data::publishing::sweep::Sweep;
use postit_jobs::maintenance::VaultRotate;
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::dto_admin::{
    AdminPlatformDto, PublishingSettingsDto, PutUserLimitsRequest, SetLivePublishingRequest,
    SetPlatformEnabledRequest, UserLimitsDto, VaultRotateResponse,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{RequestAudit, RequireAdmin};
use crate::json::{ApiJson, ApiPath};
use crate::responses::CommonProblems;
use crate::state::AppState;

/// Records `event` by `admin` in `tx`.
async fn record(
    tx: &mut PgConnection,
    admin: UserId,
    audit: AuditContext,
    kind: AuditEventKind,
    details: Vec<(&'static str, serde_json::Value)>,
    target: Option<UserId>,
) -> Result<(), ApiError> {
    let mut event = AuditEvent::new(kind).actor(admin).context(audit);
    if let Some(target) = target {
        event = event
            .detail_user_id("target_user_id", target)
            .map_err(|e| ApiError::internal(&e))?;
    }
    for (key, value) in details {
        event = event
            .detail(key, value)
            .map_err(|e| ApiError::internal(&e))?;
    }
    AuditLog::record(tx, AuditEventId::from(Uuid::now_v7()), event)
        .await
        .map_err(|e| ApiError::internal(&e))
}

async fn begin(state: &AppState) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::from(DataError::from(e)))
}

fn platform_id(text: &str) -> Result<PlatformId, ApiError> {
    PlatformId::new(text).map_err(|_| ApiError::new(ErrorCode::NotFound))
}

/// Every platform: whether this build has it, whether the deployment configures it, whether it
/// is on, and for OAuth platforms the redirect URI to register with the platform.
///
/// # Errors
///
/// 403 for non-admins.
#[utoipa::path(get, path = "/api/v1/admin/platforms", operation_id = "admin_list_platforms", tag = "admin", security(("oidc" = [])),
    responses((status = 200, body = Vec<AdminPlatformDto>),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn list_platforms(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> Result<Json<Vec<AdminPlatformDto>>, ApiError> {
    Ok(Json(
        state
            .accounts
            .admin_platforms()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Switches a platform on or off, and records `platform_enabled_changed`.
///
/// # Errors
///
/// 403 for non-admins; 404 for an unknown platform.
#[utoipa::path(put, path = "/api/v1/admin/platforms/{id}", operation_id = "admin_set_platform_enabled", tag = "admin", security(("oidc" = [])),
    params(("id" = String, Path, description = "Platform ID")),
    request_body = SetPlatformEnabledRequest,
    responses((status = 200, body = AdminPlatformDto),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn set_platform_enabled(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SetPlatformEnabledRequest>,
) -> Result<Json<AdminPlatformDto>, ApiError> {
    let id = platform_id(&id)?;
    let mut tx = begin(&state).await?;
    let old = PlatformRepo::set_enabled_in(&mut tx, &id, body.enabled)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    record(
        &mut tx,
        admin.user_id,
        audit,
        AuditEventKind::PlatformEnabledChanged,
        vec![
            ("platform_id", json!(id.as_str())),
            ("old", json!(old)),
            ("new", json!(body.enabled)),
        ],
        None,
    )
    .await?;
    tx.commit().await.map_err(DataError::from)?;
    let view = state
        .accounts
        .admin_platforms()
        .await?
        .into_iter()
        .find(|view| view.id == id)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    Ok(Json(view.into()))
}

async fn publishing_settings(state: &AppState) -> Result<PublishingSettingsDto, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let live = AppSettingsRepo::live_publishing(&mut conn).await?;
    // The same count the doctor reports, summed over the platforms.
    let native_scheduled_count = Sweep::native_scheduled_counts(&mut conn)
        .await?
        .into_iter()
        .map(|(_, count)| count)
        .sum();
    Ok(PublishingSettingsDto {
        live,
        native_scheduled_count,
    })
}

/// Whether live publishing is on, and how many natively scheduled uploads the platforms still
/// hold, which the switch cannot stop.
///
/// # Errors
///
/// 403 for non-admins.
#[utoipa::path(get, path = "/api/v1/admin/settings/publishing", operation_id = "admin_get_publishing_settings", tag = "admin", security(("oidc" = [])),
    responses((status = 200, body = PublishingSettingsDto),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_publishing(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> Result<Json<PublishingSettingsDto>, ApiError> {
    Ok(Json(publishing_settings(&state).await?))
}

/// Turns live publishing on or off, and records `live_publishing_changed`.
///
/// # Errors
///
/// 403 for non-admins.
#[utoipa::path(put, path = "/api/v1/admin/settings/publishing", operation_id = "admin_set_publishing_settings", tag = "admin", security(("oidc" = [])),
    request_body = SetLivePublishingRequest,
    responses((status = 200, body = PublishingSettingsDto),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn set_publishing(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiJson(body): ApiJson<SetLivePublishingRequest>,
) -> Result<Json<PublishingSettingsDto>, ApiError> {
    let mut tx = begin(&state).await?;
    let old = AppSettingsRepo::lock_live_publishing(&mut tx).await?;
    AppSettingsRepo::set_live_publishing(&mut tx, body.live, admin.user_id).await?;
    record(
        &mut tx,
        admin.user_id,
        audit,
        AuditEventKind::LivePublishingChanged,
        vec![("old", json!(old)), ("new", json!(body.live))],
        None,
    )
    .await?;
    tx.commit().await.map_err(DataError::from)?;
    Ok(Json(publishing_settings(&state).await?))
}

/// A user's limits. `0` means unlimited.
///
/// # Errors
///
/// 403 for non-admins; 404 for an unknown user.
#[utoipa::path(get, path = "/api/v1/admin/users/{id}/limits", operation_id = "admin_get_user_limits", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "User ID")),
    responses((status = 200, body = UserLimitsDto),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_limits(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<UserLimitsDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    Ok(Json(
        UserLimitsRepo::get(&mut conn, UserId::from(id))
            .await?
            .into(),
    ))
}

/// Replaces a user's three limits, and records `user_limits_changed` with each changed field's
/// old and new number. `0` means unlimited; a negative number is `validation_failed`.
///
/// # Errors
///
/// 403 for non-admins; 404 for an unknown user; 422 for a negative or non-numeric value.
#[utoipa::path(put, path = "/api/v1/admin/users/{id}/limits", operation_id = "admin_put_user_limits", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "User ID")),
    request_body = PutUserLimitsRequest,
    responses((status = 200, body = UserLimitsDto),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn put_limits(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PutUserLimitsRequest>,
) -> Result<Json<UserLimitsDto>, ApiError> {
    let user = UserId::from(id);
    let mut tx = begin(&state).await?;
    let old = UserLimitsRepo::lock(&mut tx, user).await?;
    let new = UserLimits {
        email_daily_cap: body.email_daily_cap,
        contacts_cap: body.contacts_cap,
        storage_quota_bytes: body.storage_quota_bytes,
        ..old
    };
    UserLimitsRepo::update(&mut tx, user, new, admin.user_id).await?;
    let mut changed = Vec::new();
    if old.email_daily_cap != new.email_daily_cap {
        changed.push((
            "email_daily_cap",
            json!({"old": old.email_daily_cap, "new": new.email_daily_cap}),
        ));
    }
    if old.contacts_cap != new.contacts_cap {
        changed.push((
            "contacts_cap",
            json!({"old": old.contacts_cap, "new": new.contacts_cap}),
        ));
    }
    if old.storage_quota_bytes != new.storage_quota_bytes {
        changed.push((
            "storage_quota_bytes",
            json!({"old": old.storage_quota_bytes, "new": new.storage_quota_bytes}),
        ));
    }
    if !changed.is_empty() {
        record(
            &mut tx,
            admin.user_id,
            audit,
            AuditEventKind::UserLimitsChanged,
            changed,
            Some(user),
        )
        .await?;
    }
    let stored = UserLimitsRepo::get(&mut tx, user).await?;
    tx.commit().await.map_err(DataError::from)?;
    Ok(Json(stored.into()))
}

/// Queues a vault key rotation: every sealed value is moved to the newest key version the
/// deployment holds. The job writes its own audit events. A rotation already queued or running
/// is `rotation_in_progress`.
///
/// # Errors
///
/// 403 for non-admins; 409 `rotation_in_progress`.
#[utoipa::path(post, path = "/api/v1/admin/vault/rotate", operation_id = "admin_rotate_vault", tag = "admin", security(("oidc" = [])),
    responses((status = 202, body = VaultRotateResponse),
        (status = 403, description = "`forbidden` (not an admin), `owner_only` (`X-Postit-Act-As` sent), `account_disabled`, or `account_pending`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`rotation_in_progress`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn rotate_vault(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
) -> Result<(StatusCode, Json<VaultRotateResponse>), ApiError> {
    let queued = state
        .jobs
        .enqueue_unless_active(&VaultRotate {
            requested_by_user_id: admin.user_id.as_uuid(),
        })
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let id = queued.ok_or_else(|| ApiError::new(ErrorCode::RotationInProgress))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(VaultRotateResponse { job_id: id.0 }),
    ))
}
