//! Admin job console: read endpoints and the audited actions.

use axum::Json;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use chrono::{DateTime, Utc};
use postit_core::{AuditEventId, UserId};
use postit_data::OwnerScope;
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::jobs::JobFilter;
use postit_jobs::ConsoleError;
use serde::Deserialize;
use sqlx::PgConnection;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_jobs::{
    JobActionResponse, JobDetailDto, JobDto, JobStateDto, JobStatsDto, RecurringJobDto,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{RequestAudit, RequireAdmin};
use crate::idempotency::{IdempotencyKey, StoredResponse, request_hash, run};
use crate::json::{ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

impl From<ConsoleError> for ApiError {
    fn from(err: ConsoleError) -> Self {
        match err {
            ConsoleError::NotFound => Self::new(ErrorCode::NotFound),
            ConsoleError::NotAllowed(reason) => {
                Self::new(ErrorCode::JobActionNotAllowed).with_detail(reason)
            }
            other => Self::internal(&other),
        }
    }
}

/// Query parameters of the job listing.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct JobListQuery {
    /// `queued`, `scheduled`, `running`, `succeeded`, `failed`, `dead`, `killed` or `cancelled`.
    #[param(value_type = Option<JobStateDto>)]
    pub state: Option<String>,
    /// Only jobs of this type.
    pub job_type: Option<String>,
    /// Inclusive lower bound (RFC 3339) on the state's relevant timestamp.
    pub from: Option<DateTime<Utc>>,
    /// Inclusive upper bound (RFC 3339) on the state's relevant timestamp.
    pub to: Option<DateTime<Utc>>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Query parameters of the stats endpoint.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct JobStatsQuery {
    /// Throughput window in hours, 1-168 (default 24).
    pub window_hours: Option<u32>,
}

/// Lists jobs, newest first, with optional filters.
///
/// # Errors
///
/// Returns 403 for non-admins and 422 for invalid filters or paging.
#[utoipa::path(get, path = "/api/v1/admin/jobs", operation_id = "list_jobs", tag = "admin", security(("oidc" = [])),
    params(JobListQuery), responses((status = 200, body = Page<JobDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<JobListQuery>,
) -> Result<Json<Page<JobDto>>, ApiError> {
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
    if let Some(name) = q.state.as_deref()
        && JobStateDto::parse(name).is_none()
    {
        return Err(ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown job state"));
    }
    let filter = JobFilter {
        state: q.state,
        job_type: q.job_type,
        from: q.from,
        to: q.to,
    };
    let result = state.console.list(filter, pagination).await?;
    let data = result.data.into_iter().map(JobDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}

/// Job counts per state, type and queue, and attempt throughput over a window.
///
/// # Errors
///
/// Returns 403 for non-admins and 422 for a window outside 1-168 hours.
#[utoipa::path(get, path = "/api/v1/admin/jobs/stats", operation_id = "get_job_stats", tag = "admin", security(("oidc" = [])),
    params(JobStatsQuery), responses((status = 200, body = JobStatsDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn stats(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<JobStatsQuery>,
) -> Result<Json<JobStatsDto>, ApiError> {
    let hours = q.window_hours.unwrap_or(24);
    if !(1..=168).contains(&hours) {
        return Err(ApiError::new(ErrorCode::ValidationFailed)
            .with_detail("`window_hours` must be between 1 and 168"));
    }
    Ok(Json(state.console.stats(hours).await?.into()))
}

/// One job with its attempt history.
///
/// # Errors
///
/// Returns 403 for non-admins and 404 for an unknown job.
#[utoipa::path(get, path = "/api/v1/admin/jobs/{id}", operation_id = "get_job", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")), responses((status = 200, body = JobDetailDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<JobDetailDto>, ApiError> {
    Ok(Json(state.console.detail(id).await?.into()))
}

/// The recurring jobs with their last and next runs.
///
/// # Errors
///
/// Returns 403 for non-admins.
#[utoipa::path(get, path = "/api/v1/admin/jobs/recurring", operation_id = "list_recurring_jobs", tag = "admin", security(("oidc" = [])),
    responses((status = 200, body = Vec<RecurringJobDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn recurring(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> Result<Json<Vec<RecurringJobDto>>, ApiError> {
    let rows = state.console.recurring().await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Records an audit event for a console action in the caller's transaction, so a refused or
/// failed action leaves no event behind. `extra` adds the retry's `new_job_id` or the
/// trigger's recurring `name`.
async fn record(
    tx: &mut PgConnection,
    actor: UserId,
    audit: AuditContext,
    kind: AuditEventKind,
    job_id: Uuid,
    job_type: &str,
    extra: Option<(&'static str, String)>,
) -> Result<(), ApiError> {
    let mut event = AuditEvent::new(kind)
        .actor(actor)
        .context(audit)
        .detail("job_id", job_id.to_string())
        .and_then(|e| e.detail("job_type", job_type))
        .map_err(|e| ApiError::internal(&e))?;
    if let Some((key, value)) = extra {
        event = event
            .detail(key, value)
            .map_err(|e| ApiError::internal(&e))?;
    }
    AuditLog::record(tx, AuditEventId::from(Uuid::now_v7()), event)
        .await
        .map_err(|e| ApiError::internal(&e))
}

/// Re-runs a failed, dead, killed or cancelled job as a new job.
///
/// # Errors
///
/// Returns 403 for non-admins, 404 for an unknown job and 409 when the job's state or type
/// does not allow a retry (`job_action_not_allowed`).
#[utoipa::path(post, path = "/api/v1/admin/jobs/{id}/retry", operation_id = "retry_job", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")), responses((status = 200, body = JobActionResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`job_action_not_allowed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn retry(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<JobActionResponse>, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let job_type = state.console.job_type_of(&mut tx, id).await?;
    let new_id = state.console.retry(&mut tx, id).await?;
    record(
        &mut tx,
        admin.user_id,
        audit,
        AuditEventKind::JobRetried,
        id,
        &job_type,
        Some(("new_job_id", new_id.to_string())),
    )
    .await?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(JobActionResponse { job_id: new_id }))
}

/// Cancels a queued or scheduled job before it runs.
///
/// # Errors
///
/// Returns 403 for non-admins, 404 for an unknown job and 409 when the job is not queued
/// (`job_action_not_allowed`).
#[utoipa::path(post, path = "/api/v1/admin/jobs/{id}/cancel", operation_id = "cancel_job", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")), responses((status = 204, description = "Cancelled"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`job_action_not_allowed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn cancel(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let job_type = state.console.job_type_of(&mut tx, id).await?;
    state.console.cancel(&mut tx, id).await?;
    record(
        &mut tx,
        admin.user_id,
        audit,
        AuditEventKind::JobCancelled,
        id,
        &job_type,
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a finished job and its attempt history.
///
/// # Errors
///
/// Returns 403 for non-admins, 404 for an unknown job and 409 when the job has not finished
/// (`job_action_not_allowed`).
#[utoipa::path(delete, path = "/api/v1/admin/jobs/{id}", operation_id = "delete_job", tag = "admin", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")), responses((status = 204, description = "Deleted"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`job_action_not_allowed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    // Read the type first: `delete` removes the row it would be read from.
    let job_type = state.console.job_type_of(&mut tx, id).await?;
    state.console.delete(&mut tx, id).await?;
    record(
        &mut tx,
        admin.user_id,
        audit,
        AuditEventKind::JobDeleted,
        id,
        &job_type,
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Runs a recurring job now, outside its schedule.
///
/// With an `Idempotency-Key`, a repeated request (a client or network retry) answers with
/// the first run's response instead of starting a second run.
///
/// # Errors
///
/// Returns 403 for non-admins and 404 for a name that is not a recurring job.
#[utoipa::path(post, path = "/api/v1/admin/jobs/recurring/{name}/trigger", operation_id = "trigger_recurring_job", tag = "admin", security(("oidc" = [])),
    params(("name" = String, Path, description = "Recurring job name"), ("Idempotency-Key" = Option<String>, Header, description = "Replays the first response for a repeated key instead of starting a second run (24 h)")), responses((status = 202, body = JobActionResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `forbidden` (not an admin)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`idempotency_in_progress`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`idempotency_key_reused` or `validation_failed` (malformed key)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn trigger(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    RequestAudit(audit): RequestAudit,
    IdempotencyKey(key): IdempotencyKey,
    ApiPath(name): ApiPath<String>,
) -> Result<StoredResponse, ApiError> {
    let route = format!("/api/v1/admin/jobs/recurring/{name}/trigger");
    let hash = request_hash(&Method::POST, &route, &[]);
    let scope = OwnerScope::own(admin.user_id);
    let pool = state.pool.clone();
    run(
        &state.pool,
        state.clock.as_ref(),
        &scope,
        key.as_deref(),
        &route,
        &hash,
        || async move {
            let mut tx = pool.begin().await.map_err(|e| ApiError::internal(&e))?;
            let job_id = state.console.trigger(&mut tx, &name).await?;
            record(
                &mut tx,
                admin.user_id,
                audit,
                AuditEventKind::JobTriggered,
                job_id,
                &name,
                Some(("name", name.clone())),
            )
            .await?;
            tx.commit().await.map_err(|e| ApiError::internal(&e))?;
            Ok(StoredResponse {
                status: StatusCode::ACCEPTED,
                body: serde_json::to_value(JobActionResponse { job_id })
                    .map_err(|e| ApiError::internal(&e))?,
            })
        },
    )
    .await
}
