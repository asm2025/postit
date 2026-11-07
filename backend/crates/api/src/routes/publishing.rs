//! Publishing: publish a post, and read, reschedule, cancel, retry, and resolve its jobs and
//! deliveries. Thin handlers over [`postit_publishing::PublishingService`]; every mutating
//! route spends the per-user `rate_limit.publish_actions` bucket after its capability check.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::{AccountId, DeliveryId, PostId, PostVersion, PublishJobId};
use postit_data::publishing::direct::DirectRepo;
use postit_data::publishing::{DeliveryRepo, JobFilter, JobRepo, Operation};
use postit_data::{Capability, DataError, OwnerScope};
use postit_publishing::PublishInput;
use uuid::Uuid;

use crate::dto_publishing::{
    CalendarDto, CalendarItemDto, CalendarQuery, CancelReportDto, DeliveryDto, JobListQuery,
    PublishJobDto, PublishJobSummaryDto, PublishRequest, RescheduleOutcomeDto, RescheduleRequest,
    ResolveRequest, RetryRequest,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{RequestAudit, Scope};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::preconditions::{ETag, check_if_match, content_etag, if_match_version, if_none_match};
use crate::responses::CommonProblems;
use crate::routes::posts::tagged_json;
use crate::routes::schedule::resolve_input;
use crate::state::AppState;

/// A `validation_failed` carrying `violations`.
fn violations_error(violations: Vec<postit_core::Violation>) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed).with_extension(
        "violations",
        serde_json::to_value(violations).unwrap_or_default(),
    )
}

/// A job with its deliveries inside the caller's limit and the progress of its direct sends.
async fn load_job(
    state: &AppState,
    scope: &OwnerScope,
    id: PublishJobId,
) -> Result<PublishJobDto, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let job = JobRepo::get(&mut conn, scope, id).await?;
    let deliveries = DeliveryRepo::list_for_job(&mut conn, scope, id).await?;
    let mut out = Vec::with_capacity(deliveries.len());
    for delivery in deliveries {
        let direct = if delivery.operation == Operation::Direct {
            Some(DirectRepo::progress(&mut conn, delivery.id).await?)
        } else {
            None
        };
        out.push(DeliveryDto::new(delivery, direct));
    }
    Ok(PublishJobDto {
        job: PublishJobSummaryDto::from(job),
        deliveries: out,
    })
}

/// A delivery the caller can see, with the progress of a direct send.
async fn load_delivery(
    state: &AppState,
    scope: &OwnerScope,
    id: DeliveryId,
) -> Result<DeliveryDto, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let delivery = DeliveryRepo::get(&mut conn, scope, id).await?;
    let direct = if delivery.operation == Operation::Direct {
        Some(DirectRepo::progress(&mut conn, delivery.id).await?)
    } else {
        None
    };
    Ok(DeliveryDto::new(delivery, direct))
}

/// The job's DTO, serialized, with its `ETag` (`"v{version}-{hash}"`: direct-send progress
/// moves without a version change, so the hash over the answer keeps a 304 honest).
fn tagged_job(job: &PublishJobDto) -> Result<(HeaderValue, Vec<u8>), ApiError> {
    let body = serde_json::to_vec(job).map_err(|e| ApiError::internal(&e))?;
    Ok((ETag(job.job.version).of_body(&body), body))
}

fn job_response(status: StatusCode, job: &PublishJobDto) -> Result<Response, ApiError> {
    let (tag, body) = tagged_job(job)?;
    Ok(tagged_json(status, tag, body))
}

/// The job version `If-Match` names; only `*` or a list of tags reads the current version to
/// check against. The service refuses a stale one.
async fn expected_job_version(
    state: &AppState,
    scope: &OwnerScope,
    id: PublishJobId,
    headers: &HeaderMap,
) -> Result<i64, ApiError> {
    if let Some(version) = if_match_version(headers)? {
        return Ok(version);
    }
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let current = JobRepo::get(&mut conn, scope, id).await?.version;
    check_if_match(headers, ETag(current))?;
    Ok(current)
}

/// Publishes a saved post to `targets`, now or at `at`. A dry run needs `edit` and only
/// simulates; a live publish needs `publish` and `confirm: true`. Needs `If-Match` with the
/// post's `ETag`, so what was reviewed is what goes out. Answers 201 with the job.
///
/// # Errors
///
/// 400 `confirmation_required`; 403 `account_not_in_scope` for a target outside a delegate's
/// limit; 404; 409 `post_locked`, `job_active`, `platform_disabled`; 412 `version_conflict`;
/// 422 `validation_failed` with every violation; 428 `precondition_required`; 503
/// `platform_unavailable`.
#[utoipa::path(post, path = "/api/v1/posts/{id}/publish", operation_id = "publish_post", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID")),
    request_body = PublishRequest,
    responses((status = 201, body = PublishJobDto, headers(("ETag" = String, description = "`\"v{job.version}-{hash}\"`"), ("Location" = String, description = "`/api/v1/publish-jobs/{id}`"))),
        (status = 400, description = "`confirmation_required`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`post_locked`, `job_active`, or `platform_disabled`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `violations`)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn publish(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PublishRequest>,
) -> Result<Response, ApiError> {
    scope.require(if body.dry_run {
        Capability::Edit
    } else {
        Capability::Publish
    })?;
    state.limits.publish.charge(&scope.actor)?;
    let id = PostId::from(id);
    let current = if let Some(version) = if_match_version(&headers)? {
        version
    } else {
        let current = state.publishing.posts().get(&scope, id).await?.post.version;
        check_if_match(&headers, ETag(current))?;
        current
    };
    let targets: Vec<AccountId> = body.targets.into_iter().map(AccountId::from).collect();
    let at = match resolve_input(&state, &scope, body.at.as_ref()).await? {
        Ok(at) => at,
        Err(violation) => {
            // A schedule that does not resolve is refused with every other violation, as the
            // service reports a schedule in the past.
            let report = state
                .publishing
                .posts()
                .validate_post(&scope, id, Some(targets), None, body.native_schedule)
                .await?;
            let mut all = vec![violation];
            all.extend(report.into_violations());
            return Err(violations_error(all));
        }
    };
    let created = state
        .publishing
        .create_job(
            &scope,
            id,
            PublishInput {
                targets,
                at,
                native_schedule: body.native_schedule,
                dry_run: body.dry_run,
                confirm: body.confirm,
                if_match: PostVersion(current),
            },
            audit,
        )
        .await?;
    let job = load_job(&state, &scope, created.job).await?;
    let location = HeaderValue::from_str(&format!("/api/v1/publish-jobs/{}", job.job.id)).ok();
    let mut response = job_response(StatusCode::CREATED, &job)?;
    if let Some(location) = location {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// The owner's publish jobs, newest first, without their deliveries. A delegate with a
/// limited account list sees a job when one of its deliveries is inside the limit. Needs `view`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/publish-jobs", operation_id = "list_publish_jobs", tag = "publishing", security(("oidc" = [])),
    params(JobListQuery),
    responses((status = 200, body = Page<PublishJobSummaryDto>),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn list_jobs(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<JobListQuery>,
) -> Result<Json<Page<PublishJobSummaryDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let filter = JobFilter {
        post_id: q.post_id.map(PostId::from),
        status: q.status,
        campaign_id: q.campaign_id.map(postit_core::CampaignId::from),
        dry_run: q.dry_run,
        from: q.from,
        to: q.to,
    };
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let page = JobRepo::list(&mut conn, &scope, pagination, filter).await?;
    Ok(Json(Page::new(
        page.data
            .into_iter()
            .map(PublishJobSummaryDto::from)
            .collect(),
        page.total,
        &pagination,
    )))
}

/// One job with the deliveries the caller can see and the progress of direct sends. Needs
/// `view`. `If-None-Match` with the current `ETag` answers 304.
///
/// # Errors
///
/// 404 for a missing job, or one with no delivery inside a delegate's limit.
#[utoipa::path(get, path = "/api/v1/publish-jobs/{id}", operation_id = "get_publish_job", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")),
    responses((status = 200, body = PublishJobDto, headers(("ETag" = String, description = "`\"v{version}-{hash}\"`"))),
        (status = 304, description = "`If-None-Match` matched"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_job(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let job = load_job(&state, &scope, PublishJobId::from(id)).await?;
    let (tag, body) = tagged_job(&job)?;
    if if_none_match(&headers, &tag) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    Ok(tagged_json(StatusCode::OK, tag, body))
}

/// Moves a job that has not started to a new time. Needs `publish`, the whole job inside the
/// caller's limit, and `If-Match` with the job's `ETag`.
///
/// # Errors
///
/// 403 `account_not_in_scope`; 404; 409 `not_reschedulable`; 412; 422 (a schedule in the past
/// or one that does not resolve); 428.
#[utoipa::path(patch, path = "/api/v1/publish-jobs/{id}", operation_id = "reschedule_publish_job", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")),
    request_body = RescheduleRequest,
    responses((status = 200, body = RescheduleOutcomeDto),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`not_reschedulable`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `violations`)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn reschedule(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RescheduleRequest>,
) -> Result<Json<RescheduleOutcomeDto>, ApiError> {
    scope.require(Capability::Publish)?;
    state.limits.publish.charge(&scope.actor)?;
    let id = PublishJobId::from(id);
    let current = expected_job_version(&state, &scope, id, &headers).await?;
    let at = resolve_input(&state, &scope, Some(&body.at))
        .await?
        .map_err(|violation| violations_error(vec![violation]))?
        .ok_or_else(|| ApiError::new(ErrorCode::ValidationFailed))?;
    let outcome = state
        .publishing
        .reschedule(&scope, id, at, current, audit)
        .await?;
    Ok(Json(outcome.into()))
}

/// Cancels every delivery of a job that has not ended. Needs `publish` and the whole job
/// inside the caller's limit.
///
/// # Errors
///
/// 403 `account_not_in_scope`; 404.
#[utoipa::path(post, path = "/api/v1/publish-jobs/{id}/cancel", operation_id = "cancel_publish_job", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Job ID")),
    responses((status = 200, body = CancelReportDto),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn cancel_job(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<CancelReportDto>, ApiError> {
    scope.require(Capability::Publish)?;
    state.limits.publish.charge(&scope.actor)?;
    let report = state
        .publishing
        .cancel_job(&scope, PublishJobId::from(id), audit)
        .await?;
    Ok(Json(report.into()))
}

/// One delivery: its state, why it is parked, the platform's ID and URL, the last error's code
/// and class (never a platform's message), and the progress of a direct send. Needs `view`.
///
/// # Errors
///
/// 404 for a missing delivery or one on an account outside a delegate's limit.
#[utoipa::path(get, path = "/api/v1/deliveries/{id}", operation_id = "get_delivery", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delivery ID")),
    responses((status = 200, body = DeliveryDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_delivery(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<DeliveryDto>, ApiError> {
    Ok(Json(
        load_delivery(&state, &scope, DeliveryId::from(id)).await?,
    ))
}

/// Cancels one delivery. Needs `publish` and the delivery's account inside the caller's limit.
///
/// # Errors
///
/// 404 for a missing delivery or one outside a delegate's limit.
#[utoipa::path(post, path = "/api/v1/deliveries/{id}/cancel", operation_id = "cancel_delivery", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delivery ID")),
    responses((status = 200, body = CancelReportDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn cancel_delivery(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<CancelReportDto>, ApiError> {
    scope.require(Capability::Publish)?;
    state.limits.publish.charge(&scope.actor)?;
    let report = state
        .publishing
        .cancel_delivery(&scope, DeliveryId::from(id), audit)
        .await?;
    Ok(Json(report.into()))
}

/// Sends a failed or stuck delivery again. Needs `publish`, `confirm: true`, and, for a
/// delivery whose outcome is unknown, `acknowledge_outcome_unknown: true`: the post may already
/// be live. Answers the delivery.
///
/// # Errors
///
/// 400 `confirmation_required` or `acknowledgment_required`; 404; 409 `not_retryable`,
/// `job_active`.
#[utoipa::path(post, path = "/api/v1/deliveries/{id}/retry", operation_id = "retry_delivery", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delivery ID")),
    request_body = RetryRequest,
    responses((status = 200, body = DeliveryDto),
        (status = 400, description = "`confirmation_required` or `acknowledgment_required`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`not_retryable` or `job_active`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn retry_delivery(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<RetryRequest>,
) -> Result<Json<DeliveryDto>, ApiError> {
    scope.require(Capability::Publish)?;
    state.limits.publish.charge(&scope.actor)?;
    let id = DeliveryId::from(id);
    state
        .publishing
        .retry(
            &scope,
            id,
            body.confirm,
            body.acknowledge_outcome_unknown,
            audit,
        )
        .await?;
    Ok(Json(load_delivery(&state, &scope, id).await?))
}

/// Settles a delivery whose outcome nobody can verify: `published` if the post is live, `failed`
/// if it is not and will not be retried. Needs `publish`. Answers the delivery.
///
/// # Errors
///
/// 404; 409 `not_resolvable` for a delivery in a state the caller's word cannot settle.
#[utoipa::path(post, path = "/api/v1/deliveries/{id}/resolve", operation_id = "resolve_delivery", tag = "publishing", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delivery ID")),
    request_body = ResolveRequest,
    responses((status = 200, body = DeliveryDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`not_resolvable`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn resolve_delivery(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ResolveRequest>,
) -> Result<Json<DeliveryDto>, ApiError> {
    scope.require(Capability::Publish)?;
    state.limits.publish.charge(&scope.actor)?;
    let id = DeliveryId::from(id);
    state
        .publishing
        .resolve(&scope, id, body.outcome, audit)
        .await?;
    Ok(Json(load_delivery(&state, &scope, id).await?))
}

/// The longest range a calendar answers: a month view plus its spill-over weeks.
const CALENDAR_MAX_DAYS: i64 = 62;

/// Live jobs due in `[from, to)`, soonest first, with their campaign color, the first 80
/// graphemes of the post's first line, and the accounts the caller can see. Dry runs are not
/// plans and are left out; so are jobs with no delivery inside a delegate's limit. Needs `view`.
/// The `ETag` is a hash of the body; `If-None-Match` with it answers 304.
///
/// # Errors
///
/// 422 `validation_failed` (`field: "to"`) when `to` is not after `from` or the range is
/// longer than 62 days.
#[utoipa::path(get, path = "/api/v1/calendar", operation_id = "get_calendar", tag = "publishing", security(("oidc" = [])),
    params(CalendarQuery),
    responses((status = 200, body = CalendarDto, headers(("ETag" = String, description = "Hash of the body"))),
        (status = 304, description = "`If-None-Match` matched"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn calendar(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<CalendarQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let span = q.to.signed_duration_since(q.from);
    if span <= chrono::Duration::zero() || span > chrono::Duration::days(CALENDAR_MAX_DAYS) {
        return Err(ApiError::new(ErrorCode::ValidationFailed).with_extension("field", "to"));
    }
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let jobs = JobRepo::calendar(&mut conn, &scope, q.from, q.to).await?;
    let body = CalendarDto {
        items: jobs.into_iter().map(CalendarItemDto::from).collect(),
    };
    let bytes = serde_json::to_vec(&body).map_err(|e| ApiError::internal(&e))?;
    let tag = content_etag(&bytes);
    if if_none_match(&headers, &tag) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    Ok((
        [
            (header::ETAG, tag),
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            ),
        ],
        bytes,
    )
        .into_response())
}
