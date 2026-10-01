use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::audit::AuditEventKind;
use postit_data::audit_repo::{AuditFilter, AuditRepo};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto::AuditEventDto;
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::RequireAdmin;
use crate::json::ApiQuery;
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
    /// An audit event kind, e.g. `user_approved`.
    pub kind: Option<String>,
    /// Inclusive lower bound (RFC 3339).
    pub from: Option<DateTime<Utc>>,
    /// Inclusive upper bound (RFC 3339).
    pub to: Option<DateTime<Utc>>,
    pub actor_user_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Lists audit events, newest first, with optional filters.
///
/// # Errors
///
/// Returns 403 for non-admins and 422 for invalid filters or paging.
#[utoipa::path(get, path = "/api/v1/admin/audit", operation_id = "list_audit_events", tag = "admin", security(("oidc" = [])),
    params(AuditQuery), responses((status = 200, body = Page<AuditEventDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<AuditQuery>,
) -> Result<Json<Page<AuditEventDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    if let (Some(from), Some(to)) = (q.from, q.to)
        && from > to
    {
        return Err(
            ApiError::new(ErrorCode::ValidationFailed).with_detail("`from` must not be after `to`")
        );
    }
    let kind = q
        .kind
        .as_deref()
        .map(str::parse::<AuditEventKind>)
        .transpose()
        .map_err(|_| {
            ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown audit event kind")
        })?;
    let filter = AuditFilter {
        kind: kind.map(|k| k.as_str().to_string()),
        from: q.from,
        to: q.to,
        actor_user_id: q.actor_user_id.map(UserId::from),
        subject_user_id: q.subject_user_id.map(UserId::from),
    };
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let result = AuditRepo::list(&mut conn, &filter, pagination).await?;
    let data = result.data.into_iter().map(AuditEventDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}
