//! The owner's workspace content: the brand profile and writing styles (this file's first
//! half), then target groups and campaigns. Thin handlers over the `postit-data` repositories,
//! which apply the capability and the account limit.

use axum::Json;
use axum::extract::State;
use postit_core::{AccountId, BrandProfileId, WritingStyleId};
use postit_data::DataError;
use postit_data::brand_profiles::BrandProfileRepo;
use postit_data::writing_styles::WritingStyleRepo;
use uuid::Uuid;

use crate::dto_workspace::{
    BrandProfileDto, PutBrandProfileRequest, PutWritingStyleRequest, WritingStyleDto,
};
use crate::error::{ApiError, ProblemDetails};
use crate::extract::Scope;
use crate::json::{ApiJson, ApiPath};
use crate::responses::CommonProblems;
use crate::state::AppState;

/// The brand profile. An owner who wrote none reads an empty profile with `id: null`, so a
/// client renders one form either way. Needs `view`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/brand-profile", operation_id = "get_brand_profile", tag = "workspace", security(("oidc" = [])),
    responses((status = 200, body = BrandProfileDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_brand_profile(
    State(state): State<AppState>,
    Scope(scope): Scope,
) -> Result<Json<BrandProfileDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    Ok(Json(BrandProfileRepo::get(&mut conn, &scope).await?.into()))
}

/// Stores the brand profile, replacing the earlier one. Needs `edit`.
///
/// # Errors
///
/// 422 `validation_failed` with `field: "examples"` for more than five examples.
#[utoipa::path(put, path = "/api/v1/brand-profile", operation_id = "put_brand_profile", tag = "workspace", security(("oidc" = [])),
    request_body = PutBrandProfileRequest,
    responses((status = 200, body = BrandProfileDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn put_brand_profile(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<PutBrandProfileRequest>,
) -> Result<Json<BrandProfileDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let stored = BrandProfileRepo::upsert(
        &mut conn,
        &scope,
        BrandProfileId::from_uuid(Uuid::now_v7()),
        body.into(),
    )
    .await?;
    Ok(Json(Some(stored).into()))
}

/// An account's writing style. An account in view that has none reads an empty style with
/// `id: null`. Needs `view`.
///
/// # Errors
///
/// 404 for an account that is missing, not the owner's, or outside a delegate's account limit.
#[utoipa::path(get, path = "/api/v1/accounts/{id}/writing-style", operation_id = "get_writing_style", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = WritingStyleDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get_writing_style(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<WritingStyleDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let style = WritingStyleRepo::find(&mut conn, &scope, AccountId::from(id)).await?;
    Ok(Json(WritingStyleDto::of(id, style)))
}

/// Stores an account's writing style, replacing the earlier one. Needs `edit`.
///
/// # Errors
///
/// 404 for an account that is missing, not the owner's, or outside a delegate's account limit.
#[utoipa::path(put, path = "/api/v1/accounts/{id}/writing-style", operation_id = "put_writing_style", tag = "workspace", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    request_body = PutWritingStyleRequest,
    responses((status = 200, body = WritingStyleDto),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn put_writing_style(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PutWritingStyleRequest>,
) -> Result<Json<WritingStyleDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    let stored = WritingStyleRepo::upsert(
        &mut conn,
        &scope,
        WritingStyleId::from_uuid(Uuid::now_v7()),
        AccountId::from(id),
        body.into(),
    )
    .await?;
    Ok(Json(WritingStyleDto::of(id, Some(stored))))
}
