//! Posts and their per-account overrides: thin handlers over [`postit_publishing::PostService`].
//! Every read and write answers the post's `ETag` (`"v{version}"`); writes need `If-Match`.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::{AccountId, AiGenerationId, PostId, PostVersion};
use postit_data::OwnerScope;
use postit_data::posts::PostFilter;
use postit_publishing::PostView;
use uuid::Uuid;

use crate::dto_posts::{
    CreatePostRequest, PatchPostRequest, PostDto, PostListQuery, PostSummaryDto, PutOverrideRequest,
};
use crate::error::{ApiError, ProblemDetails};
use crate::extract::Scope;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::preconditions::{ETag, check_if_match, if_none_match};
use crate::responses::CommonProblems;
use crate::state::AppState;

fn post_response(status: StatusCode, view: PostView) -> Response {
    let tag = ETag(view.post.version).header_value();
    (status, [(header::ETAG, tag)], Json(PostDto::from(view))).into_response()
}

/// Checks `If-Match` against the post's current version and returns that version, which the
/// service locks the post at. A write that lands between the two sees the lock refuse it.
async fn expected_version(
    state: &AppState,
    scope: &OwnerScope,
    id: PostId,
    headers: &HeaderMap,
) -> Result<PostVersion, ApiError> {
    let current = state.publishing.posts().get(scope, id).await?.post.version;
    check_if_match(headers, ETag(current))?;
    Ok(PostVersion(current))
}

/// The owner's posts, newest first, without media, targets, or overrides. Needs `view`.
///
/// # Errors
///
/// None of its own.
#[utoipa::path(get, path = "/api/v1/posts", operation_id = "list_posts", tag = "posts", security(("oidc" = [])),
    params(PostListQuery),
    responses((status = 200, body = Page<PostSummaryDto>),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<PostListQuery>,
) -> Result<Json<Page<PostSummaryDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let filter = PostFilter {
        status: q.status.map(Into::into),
        campaign_id: q.campaign_id.map(postit_core::CampaignId::from),
        kind: q.kind,
    };
    let page = state
        .publishing
        .posts()
        .list(&scope, pagination, filter)
        .await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(PostSummaryDto::from).collect(),
        page.total,
        &pagination,
    )))
}

/// Creates a draft with its media and targets. Needs `edit`. With `generation_id`, links the
/// AI generation it was saved from.
///
/// # Errors
///
/// 403 `account_not_in_scope` for a target outside a delegate's account limit; 404 for a
/// campaign, media asset, target account, or generation that is not the owner's (or a
/// generation already linked to a post); 422 `validation_failed` at `link`.
#[utoipa::path(post, path = "/api/v1/posts", operation_id = "create_post", tag = "posts", security(("oidc" = [])),
    request_body = CreatePostRequest,
    responses((status = 201, body = PostDto, headers(("ETag" = String, description = "`\"v{version}\"`"))),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn create(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(mut body): ApiJson<CreatePostRequest>,
) -> Result<Response, ApiError> {
    let generation = body.generation_id.take().map(AiGenerationId::from);
    let view = state
        .publishing
        .posts()
        .create(&scope, body.into_draft()?, generation)
        .await?;
    let location = HeaderValue::from_str(&format!("/api/v1/posts/{}", view.post.id)).ok();
    let mut response = post_response(StatusCode::CREATED, view);
    if let Some(location) = location {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// One post with its media, targets, overrides, and the newest live delivery per account.
/// Needs `view`. `If-None-Match` with the current `ETag` answers 304.
///
/// # Errors
///
/// 404 for a missing post.
#[utoipa::path(get, path = "/api/v1/posts/{id}", operation_id = "get_post", tag = "posts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID")),
    responses((status = 200, body = PostDto, headers(("ETag" = String, description = "`\"v{version}\"`"))),
        (status = 304, description = "`If-None-Match` matched"),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn get(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let view = state
        .publishing
        .posts()
        .get(&scope, PostId::from(id))
        .await?;
    let tag = ETag(view.post.version).header_value();
    if if_none_match(&headers, &tag) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    Ok(post_response(StatusCode::OK, view))
}

/// Changes a post. Needs `edit` and `If-Match`. Targets outside a limited delegate's account
/// limit stay. A post an unfinished or scheduled publish job holds is `post_locked`.
///
/// # Errors
///
/// 403 `account_not_in_scope` when an override or unfinished delivery belongs to an account
/// outside the delegate's limit; 404; 409 `post_locked`; 412 `version_conflict`; 428
/// `precondition_required`.
#[utoipa::path(patch, path = "/api/v1/posts/{id}", operation_id = "patch_post", tag = "posts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID")),
    request_body = PatchPostRequest,
    responses((status = 200, body = PostDto, headers(("ETag" = String, description = "`\"v{version}\"`"))),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`post_locked`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn patch(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PatchPostRequest>,
) -> Result<Response, ApiError> {
    let id = PostId::from(id);
    let version = expected_version(&state, &scope, id, &headers).await?;
    let view = state
        .publishing
        .posts()
        .update(&scope, id, version, body.into_patch()?)
        .await?;
    Ok(post_response(StatusCode::OK, view))
}

/// Deletes a post with its media links, targets, overrides, jobs, and deliveries. Needs `edit`,
/// and `publish` when the post has live history. Needs `If-Match`.
///
/// # Errors
///
/// As `PATCH /posts/{id}`.
#[utoipa::path(delete, path = "/api/v1/posts/{id}", operation_id = "delete_post", tag = "posts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID")),
    responses((status = 204, description = "Deleted"),
        (status = 403, description = "`capability_required` or `account_not_in_scope`, or `account_disabled`, `account_pending`, `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`post_locked`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn delete(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let id = PostId::from(id);
    let version = expected_version(&state, &scope, id, &headers).await?;
    state.publishing.posts().delete(&scope, id, version).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sets the override for one account. Needs `edit` and `If-Match`; the post's version
/// changes, and the answer carries the new `ETag`.
///
/// # Errors
///
/// As `PATCH /posts/{id}`; an account outside a delegate's limit, not the owner's, or
/// disconnected is 404.
#[utoipa::path(put, path = "/api/v1/posts/{id}/overrides/{account_id}", operation_id = "put_post_override", tag = "posts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID"), ("account_id" = Uuid, Path, description = "Account ID")),
    request_body = PutOverrideRequest,
    responses((status = 200, body = PostDto, headers(("ETag" = String, description = "`\"v{version}\"`"))),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`post_locked`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn put_override(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath((id, account)): ApiPath<(Uuid, Uuid)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PutOverrideRequest>,
) -> Result<Response, ApiError> {
    let id = PostId::from(id);
    let version = expected_version(&state, &scope, id, &headers).await?;
    let patch = serde_json::to_value(&body.patch).map_err(|e| ApiError::internal(&e))?;
    let view = state
        .publishing
        .posts()
        .put_override(
            &scope,
            id,
            AccountId::from(account),
            version,
            patch,
            serde_json::Value::Object(body.options),
        )
        .await?;
    Ok(post_response(StatusCode::OK, view))
}

/// Removes the override for one account. Needs `edit` and `If-Match`.
///
/// # Errors
///
/// As `PUT /posts/{id}/overrides/{account_id}`; an account with no override is 404.
#[utoipa::path(delete, path = "/api/v1/posts/{id}/overrides/{account_id}", operation_id = "delete_post_override", tag = "posts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Post ID"), ("account_id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = PostDto, headers(("ETag" = String, description = "`\"v{version}\"`"))),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "`post_locked`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 412, description = "`version_conflict`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 428, description = "`precondition_required`", body = ProblemDetails, content_type = "application/problem+json"),
        CommonProblems))]
pub async fn delete_override(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath((id, account)): ApiPath<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let id = PostId::from(id);
    let version = expected_version(&state, &scope, id, &headers).await?;
    let view = state
        .publishing
        .posts()
        .delete_override(&scope, id, AccountId::from(account), version)
        .await?;
    Ok(post_response(StatusCode::OK, view))
}
