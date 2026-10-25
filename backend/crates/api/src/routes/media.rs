//! The media library: uploads (one request or resumable), listing, serving, signed view URLs,
//! and deletion. Media is not bound to an account, so a delegate's account limit does not hide
//! it: reads need `view` access and writes need `edit`.

use std::io;

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::{StreamExt as _, TryStreamExt as _};
use postit_core::{ByteStream, MediaAssetId};
use postit_media::ServedFile;
use serde::Deserialize;
use tower::ServiceExt as _;
use tower_http::services::ServeFile;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_media::{
    MediaDto, MediaUsageDto, UploadOffsetDto, UploadSessionDto, UploadSessionRequest, ViewUrlsDto,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::Scope;
use crate::idempotency::{IdempotencyKey, StoredResponse, request_hash, run};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

const UPLOAD_OFFSET: HeaderName = HeaderName::from_static("upload-offset");
const UPLOAD_LENGTH: HeaderName = HeaderName::from_static("upload-length");

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct MediaListQuery {
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Query of the single-request upload.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct UploadQuery {
    /// The file name, 1 to 255 characters. Its extension (`jpg`, `png`, `gif`, `webp`, `mp4`,
    /// `m4v`, `mov`) picks the kind and so the size limit. Metadata only.
    pub filename: String,
}

fn asset_id(id: Uuid) -> MediaAssetId {
    MediaAssetId::from_uuid(id)
}

/// The request body as the byte stream the media service takes.
fn byte_stream(body: Body) -> ByteStream {
    body.into_data_stream().map_err(io::Error::other).boxed()
}

fn invalid(field: &'static str) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed).with_extension("field", field)
}

/// The declared size of a streamed body: `Content-Length` is required.
fn content_length(headers: &HeaderMap) -> Result<u64, ApiError> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .ok_or_else(|| invalid("content_length"))
}

/// `bytes {start}-{end}/{total}`, with all three numbers.
fn content_range(headers: &HeaderMap) -> Result<(u64, u64, u64), ApiError> {
    let value = headers
        .get(header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| invalid("content_range"))?;
    let parse = || {
        let range = value.trim().strip_prefix("bytes ")?;
        let (span, total) = range.split_once('/')?;
        let (start, end) = span.split_once('-')?;
        Some((
            start.trim().parse().ok()?,
            end.trim().parse().ok()?,
            total.trim().parse().ok()?,
        ))
    };
    parse().ok_or_else(|| invalid("content_range"))
}

fn json_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal(&e))
}

/// A `201` with a `Location`, built from a stored (possibly replayed) response.
fn created(stored: StoredResponse, collection: &str) -> Response {
    let location = stored
        .body
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| HeaderValue::from_str(&format!("/api/v1/{collection}/{id}")).ok());
    let mut response = stored.into_response();
    if let Some(location) = location {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}

/// Serves `file` with range and conditional-request support, as the sniffed type, sandboxed.
pub(crate) async fn serve(req: Request, file: &ServedFile) -> Result<Response, ApiError> {
    let mime: mime::Mime = file.mime.parse().map_err(|e| ApiError::internal(&e))?;
    let response = ServeFile::new_with_mime(&file.path, &mime)
        .oneshot(req)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let mut response = response.map(Body::new);
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    Ok(response)
}

/// Uploads a whole file in one request. The body is the raw file and `Content-Length` is
/// required. The stored type is what the bytes sniff as, never the request's `Content-Type`.
/// With an `Idempotency-Key`, a replay is recognized by the file name, `Content-Length`, and
/// `Content-Type` (the streamed body is not hashed).
///
/// # Errors
///
/// 413 `payload_too_large` (kind limit or single-request limit) or `storage_quota_exceeded`
/// before any byte is stored, 415 `unsupported_media_type`, 422 `validation_failed` for a bad
/// name, a missing `Content-Length`, or a body that does not match it.
#[utoipa::path(post, path = "/api/v1/media", operation_id = "upload_media", tag = "media", security(("oidc" = [])),
    params(UploadQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The raw file"),
    responses((status = 201, body = MediaDto, headers(("Location" = String, description = "`/api/v1/media/{id}`"))), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large` (see `max_bytes`) or `storage_quota_exceeded` (see `quota_bytes`, `used_bytes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 415, description = "`unsupported_media_type`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `field`) or `idempotency_key_reused`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn upload(
    State(state): State<AppState>,
    Scope(scope): Scope,
    IdempotencyKey(key): IdempotencyKey,
    ApiQuery(q): ApiQuery<UploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let declared = content_length(&headers)?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let route = "/api/v1/media";
    let hash = request_hash(
        &Method::POST,
        route,
        format!("{}\n{declared}\n{content_type}", q.filename).as_bytes(),
    );
    let stored = run(
        &state.pool,
        state.clock.as_ref(),
        &scope,
        key.as_deref(),
        route,
        &hash,
        || async {
            let media = state
                .media
                .upload(&scope, &q.filename, declared, byte_stream(body))
                .await?;
            Ok(StoredResponse {
                status: StatusCode::CREATED,
                body: json_value(&MediaDto::from(media))?,
            })
        },
    )
    .await?;
    Ok(created(stored, "media"))
}

/// Lists the owner's media, newest first.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/media", operation_id = "list_media", tag = "media", security(("oidc" = [])),
    params(MediaListQuery),
    responses((status = 200, body = Page<MediaDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<MediaListQuery>,
) -> Result<Json<Page<MediaDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let page = state.media.list(&scope, pagination).await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(MediaDto::from).collect(),
        page.total,
        &pagination,
    )))
}

/// Storage used against the quota.
///
/// # Errors
///
/// 403 without `view` access.
#[utoipa::path(get, path = "/api/v1/media/usage", operation_id = "media_usage", tag = "media", security(("oidc" = [])),
    responses((status = 200, body = MediaUsageDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn usage(
    State(state): State<AppState>,
    Scope(scope): Scope,
) -> Result<Json<MediaUsageDto>, ApiError> {
    Ok(Json(state.media.usage(&scope).await?.into()))
}

/// One asset.
///
/// # Errors
///
/// 404 for a missing asset.
#[utoipa::path(get, path = "/api/v1/media/{id}", operation_id = "get_media", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Media ID")),
    responses((status = 200, body = MediaDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<MediaDto>, ApiError> {
    Ok(Json(state.media.get(&scope, asset_id(id)).await?.into()))
}

/// Deletes an asset and its files.
///
/// # Errors
///
/// 409 `media_in_use` while a post or an unfinished job uses it.
#[utoipa::path(delete, path = "/api/v1/media/{id}", operation_id = "delete_media", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Media ID")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`media_in_use`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.media.delete(&scope, asset_id(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The file, with `Range` and conditional-request support. The type is what the bytes sniffed
/// as; the response is sandboxed and never sniffed.
///
/// # Errors
///
/// 404 for a missing or unfinished asset, 416 for a range past the end.
#[utoipa::path(get, path = "/api/v1/media/{id}/content", operation_id = "get_media_content", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Media ID")),
    responses((status = 200, description = "The file", content_type = "application/octet-stream"), (status = 206, description = "The requested range", content_type = "application/octet-stream"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 416, description = "The range is past the end of the file"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn content(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    req: Request,
) -> Result<Response, ApiError> {
    let file = state.media.content(&scope, asset_id(id)).await?;
    serve(req, &file).await
}

/// The JPEG thumbnail, longest side 480 px.
///
/// # Errors
///
/// 404 for a missing asset or one without a thumbnail.
#[utoipa::path(get, path = "/api/v1/media/{id}/thumbnail", operation_id = "get_media_thumbnail", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Media ID")),
    responses((status = 200, description = "The thumbnail", content_type = "image/jpeg"), (status = 206, description = "The requested range", content_type = "image/jpeg"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 416, description = "The range is past the end of the file"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn thumbnail(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    req: Request,
) -> Result<Response, ApiError> {
    let file = state.media.thumbnail(&scope, asset_id(id)).await?;
    serve(req, &file).await
}

/// Short-lived signed URLs for showing the asset without a bearer token, for example in an
/// `<img>` tag.
///
/// # Errors
///
/// 404 for a missing or unfinished asset.
#[utoipa::path(get, path = "/api/v1/media/{id}/view-url", operation_id = "get_media_view_url", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Media ID")),
    responses((status = 200, body = ViewUrlsDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn view_url(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<ViewUrlsDto>, ApiError> {
    Ok(Json(
        state.media.view_urls(&scope, asset_id(id)).await?.into(),
    ))
}

/// Starts a resumable upload. Send the file in chunks with `PUT /media/uploads/{id}`, ask
/// `HEAD` where it stands, and close it with `POST /media/uploads/{id}/complete`.
///
/// # Errors
///
/// The refusals of the single-request upload that need no body: 413, 415, 422.
#[utoipa::path(post, path = "/api/v1/media/uploads", operation_id = "start_media_upload", tag = "media", security(("oidc" = [])),
    request_body = UploadSessionRequest,
    responses((status = 201, body = UploadSessionDto, headers(("Location" = String, description = "`/api/v1/media/uploads/{id}`"))), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large` (see `max_bytes`) or `storage_quota_exceeded` (see `quota_bytes`, `used_bytes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 415, description = "`unsupported_media_type`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `field`) or `idempotency_key_reused`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn start_session(
    State(state): State<AppState>,
    Scope(scope): Scope,
    IdempotencyKey(key): IdempotencyKey,
    ApiJson(body): ApiJson<UploadSessionRequest>,
) -> Result<Response, ApiError> {
    let route = "/api/v1/media/uploads";
    let hash = request_hash(
        &Method::POST,
        route,
        format!("{}\n{}", body.filename, body.size).as_bytes(),
    );
    let chunk_max = u64::try_from(state.settings.media_chunk_max).unwrap_or(u64::MAX);
    let stored = run(
        &state.pool,
        state.clock.as_ref(),
        &scope,
        key.as_deref(),
        route,
        &hash,
        || async {
            let media = state
                .media
                .start_session(&scope, &body.filename, body.size)
                .await?;
            Ok(StoredResponse {
                status: StatusCode::CREATED,
                body: json_value(&UploadSessionDto::new(&media, chunk_max))?,
            })
        },
    )
    .await?;
    Ok(created(stored, "media/uploads"))
}

/// Appends a chunk. `Content-Range: bytes {start}-{end}/{total}` is required, `start` must be
/// the stored offset, and the body must be exactly `end - start + 1` bytes. After a dropped
/// connection, ask `HEAD` for the offset and resume there: bytes stored past it are cut off.
///
/// # Errors
///
/// 409 `upload_offset_mismatch` (see `offset`), 413 over the chunk limit, 422
/// `validation_failed` (`field: content_range`), 404 for a missing or expired session.
#[utoipa::path(put, path = "/api/v1/media/uploads/{id}", operation_id = "append_media_chunk", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Upload ID"), ("Content-Range" = String, Header, description = "`bytes {start}-{end}/{total}`")),
    request_body(content = String, content_type = "application/octet-stream", description = "The chunk"),
    responses((status = 200, body = UploadOffsetDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`upload_offset_mismatch` (see `offset`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large` (see `max_bytes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `field`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn append(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let (start, end, total) = content_range(&headers)?;
    let offset = state
        .media
        .append(&scope, asset_id(id), start, end, total, byte_stream(body))
        .await?;
    let mut response = Json(UploadOffsetDto { offset }).into_response();
    response
        .headers_mut()
        .insert(UPLOAD_OFFSET, HeaderValue::from(offset));
    Ok(response)
}

/// Where an upload stands: `Upload-Offset` (the next byte to send) and `Upload-Length` (the
/// declared size), with no body.
///
/// # Errors
///
/// 404 for a missing or expired session.
#[utoipa::path(head, path = "/api/v1/media/uploads/{id}", operation_id = "media_upload_status", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Upload ID")),
    responses((status = 200, description = "The upload's position", headers(("Upload-Offset" = u64, description = "Bytes stored; the next chunk starts here"), ("Upload-Length" = u64, description = "The declared size"))), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn session_head(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Response, ApiError> {
    let (offset, length) = state.media.offset(&scope, asset_id(id)).await?;
    let mut response = StatusCode::OK.into_response();
    response
        .headers_mut()
        .insert(UPLOAD_OFFSET, HeaderValue::from(offset));
    response
        .headers_mut()
        .insert(UPLOAD_LENGTH, HeaderValue::from(length));
    Ok(response)
}

/// Closes an upload whose last byte arrived: checks the bytes, reads their size, and makes the
/// thumbnail. A file that is not an accepted image or video of the kind its name claims fails
/// the upload and removes it.
///
/// # Errors
///
/// 409 `upload_offset_mismatch` (see `offset`) before the last byte, 415
/// `unsupported_media_type`, 404 for a missing or expired session.
#[utoipa::path(post, path = "/api/v1/media/uploads/{id}/complete", operation_id = "complete_media_upload", tag = "media", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Upload ID")),
    responses((status = 200, body = MediaDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`upload_offset_mismatch` (see `offset`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 415, description = "`unsupported_media_type`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn complete(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<MediaDto>, ApiError> {
    Ok(Json(
        state.media.complete(&scope, asset_id(id)).await?.into(),
    ))
}
