//! Target groups and campaigns: thin handlers over their `postit-data` repositories, which
//! apply the capability and the account limit.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::{AccountId, CampaignId, TargetGroupId};
use postit_data::DataError;
use postit_data::campaigns::CampaignRepo;
use postit_data::target_groups::TargetGroupRepo;
use sqlx::Connection as _;
use uuid::Uuid;

use crate::dto_organize::{
    CampaignDto, CampaignListQuery, CreateCampaignRequest, PatchCampaignRequest, TargetGroupDto,
    TargetGroupRequest,
};
use crate::error::{ApiError, ProblemDetails};
use crate::extract::Scope;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::responses::CommonProblems;
use crate::state::AppState;

fn account_ids(ids: &[Uuid]) -> Vec<AccountId> {
    ids.iter().copied().map(AccountId::from).collect()
}

/// The owner's target groups, by name, with the members the caller can see. Needs `view`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/target-groups", operation_id = "list_target_groups", tag = "workspace", security(("oidc" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<TargetGroupDto>),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn list_groups(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> Result<Json<Page<TargetGroupDto>>, ApiError> {
    let pagination = q.to_pagination()?;
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let page = TargetGroupRepo::list(&mut conn, &scope, pagination).await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(TargetGroupDto::from).collect(),
        page.total,
        &pagination,
    )))
}

/// Creates a target group. Needs `edit`.
///
/// # Errors
///
/// 403 `account_not_in_scope` for a member outside a delegate's account limit; 404 for an
/// account that is not the owner's.
#[utoipa::path(post, path = "/api/v1/target-groups", operation_id = "create_target_group", tag = "workspace", security(("oidc" = [])),
    request_body = TargetGroupRequest,
    responses((status = 201, body = TargetGroupDto),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn create_group(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<TargetGroupRequest>,
) -> Result<(StatusCode, Json<TargetGroupDto>), ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let group = TargetGroupRepo::create(
        &mut conn,
        &scope,
        TargetGroupId::from_uuid(Uuid::now_v7()),
        &body.name,
        &account_ids(&body.account_ids),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(group.into())))
}

/// Renames a target group and replaces its members. Members a limited delegate cannot see
/// stay. Needs `edit`.
///
/// # Errors
///
/// 404 for a missing group; 403 `account_not_in_scope` for a member outside the caller's limit.
#[utoipa::path(put, path = "/api/v1/target-groups/{id}", operation_id = "replace_target_group", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Target group ID")),
    request_body = TargetGroupRequest,
    responses((status = 200, body = TargetGroupDto),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn replace_group(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<TargetGroupRequest>,
) -> Result<Json<TargetGroupDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let group = TargetGroupRepo::replace(
        &mut conn,
        &scope,
        TargetGroupId::from_uuid(id),
        &body.name,
        &account_ids(&body.account_ids),
    )
    .await?;
    Ok(Json(group.into()))
}

/// Deletes a target group. Its accounts and the posts sent through it stay. Needs `edit`.
///
/// # Errors
///
/// 404 for a missing group.
#[utoipa::path(delete, path = "/api/v1/target-groups/{id}", operation_id = "delete_target_group", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Target group ID")),
    responses((status = 204, description = "Deleted"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn delete_group(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    TargetGroupRepo::delete(&mut conn, &scope, TargetGroupId::from_uuid(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The owner's campaigns, newest first. Archived ones appear only with `include_archived`.
/// Needs `view`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/campaigns", operation_id = "list_campaigns", tag = "workspace", security(("oidc" = [])),
    params(CampaignListQuery),
    responses((status = 200, body = Page<CampaignDto>),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn list_campaigns(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<CampaignListQuery>,
) -> Result<Json<Page<CampaignDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let page = CampaignRepo::list(
        &mut conn,
        &scope,
        pagination,
        q.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(CampaignDto::from).collect(),
        page.total,
        &pagination,
    )))
}

/// Creates a campaign. Needs `edit`.
///
/// # Errors
///
/// 422 `validation_failed` with `field: "ends_on"` when it ends before it starts.
#[utoipa::path(post, path = "/api/v1/campaigns", operation_id = "create_campaign", tag = "workspace", security(("oidc" = [])),
    request_body = CreateCampaignRequest,
    responses((status = 201, body = CampaignDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn create_campaign(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<CreateCampaignRequest>,
) -> Result<(StatusCode, Json<CampaignDto>), ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let campaign = CampaignRepo::create(
        &mut conn,
        &scope,
        CampaignId::from_uuid(Uuid::now_v7()),
        body.into(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(campaign.into())))
}

/// Changes a campaign: only the fields sent, and `null` clears a date or the color.
/// `archived` archives or restores it. Needs `edit`.
///
/// # Errors
///
/// 404 for a missing campaign; 422 `validation_failed` with `field: "ends_on"` when the result
/// ends before it starts.
#[utoipa::path(patch, path = "/api/v1/campaigns/{id}", operation_id = "patch_campaign", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Campaign ID")),
    request_body = PatchCampaignRequest,
    responses((status = 200, body = CampaignDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn patch_campaign(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PatchCampaignRequest>,
) -> Result<Json<CampaignDto>, ApiError> {
    let id = CampaignId::from_uuid(id);
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let mut tx = conn.begin().await.map_err(DataError::from)?;
    let mut campaign = CampaignRepo::patch(&mut tx, &scope, id, body.patch()).await?;
    if let Some(archived) = body.archived {
        campaign = CampaignRepo::archive(&mut tx, &scope, id, archived).await?;
    }
    tx.commit().await.map_err(DataError::from)?;
    Ok(Json(campaign.into()))
}

/// Deletes a campaign. Its posts stay and lose the campaign. Needs `edit`.
///
/// # Errors
///
/// 404 for a missing campaign.
#[utoipa::path(delete, path = "/api/v1/campaigns/{id}", operation_id = "delete_campaign", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Campaign ID")),
    responses((status = 204, description = "Deleted"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn delete_campaign(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    CampaignRepo::delete(&mut conn, &scope, CampaignId::from_uuid(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
