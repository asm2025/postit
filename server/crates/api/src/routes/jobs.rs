//! Admin job console: read endpoints.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use postit_data::jobs::JobFilter;
use postit_jobs::ConsoleError;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_jobs::{JobDetailDto, JobDto, JobStateDto, JobStatsDto, RecurringJobDto};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::RequireAdmin;
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
