//! Connected accounts: pasted-credential connection, listing, checks, and disconnecting.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use futures::TryStreamExt as _;
use postit_core::{AccountId, PlatformId};
use postit_data::OwnerScope;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::io::{AsyncRead, ReadBuf};
use tokio_util::io::StreamReader;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_accounts::{
    AccountDto, ConfirmRequest, ConfirmResponse, CredentialCheckResponse, DisconnectResponse,
    ImportReportDto, UpdateAccountRequest,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{RequestAudit, Scope};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// Charges the per-user bucket for requests that call a platform on the user's behalf.
fn charge(state: &AppState, scope: &OwnerScope) -> Result<(), ApiError> {
    state
        .limits
        .credential_checks
        .check(&scope.actor)
        .map_err(|wait| ApiError::new(ErrorCode::RateLimited).with_retry_after(wait))
}

/// A platform ID that cannot exist is a platform that is not there.
fn platform_id(text: &str) -> Result<PlatformId, ApiError> {
    PlatformId::new(text).map_err(|_| ApiError::new(ErrorCode::NotFound))
}

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AccountListQuery {
    /// Only accounts on this platform.
    pub platform: Option<String>,
    /// Include disconnected accounts (default false).
    pub include_disconnected: Option<bool>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Query of the CSV import.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// `true` connects the valid rows; `false` (the default) only checks the file.
    pub commit: Option<bool>,
}

/// Checks pasted credentials against the platform and lists the accounts they reach. The
/// body is a JSON object with the platform's credential fields. When the credential reaches
/// accounts, the response carries a one-time `connect_id` to confirm with.
///
/// # Errors
///
/// 422 `validation_failed` (every rule the values break, in `violations`) or
/// `credentials_invalid` (the field at fault, in `field`), 409 `capability_unsupported` for an
/// OAuth platform, 404 for an unknown platform, 429 when the credential-check limit is spent.
#[utoipa::path(post, path = "/api/v1/accounts/credentials/{platform}", tag = "accounts", security(("oidc" = [])),
    params(("platform" = String, Path, description = "Platform ID, such as `telegram`")),
    request_body(content = Object, description = "The credential fields the platform asks for", example = json!({"token": "<bot token>"})),
    responses((status = 200, body = CredentialCheckResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `violations`) or `credentials_invalid` (see `field`, `missing_scopes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn credentials(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(platform): ApiPath<String>,
    ApiJson(values): ApiJson<Map<String, Value>>,
) -> Result<Json<CredentialCheckResponse>, ApiError> {
    charge(&state, &scope)?;
    let check = state
        .accounts
        .start_credentials(&scope, &platform_id(&platform)?, values)
        .await?;
    Ok(Json(check.into()))
}

/// Connects the accounts the user picked from a checked credential. The `connect_id` works
/// once, for the user who checked the credential.
///
/// # Errors
///
/// 404 for an unknown, expired, used, or foreign `connect_id`; 409 `platform_disabled` or
/// `sending_method_exists`.
#[utoipa::path(post, path = "/api/v1/accounts/connect/{connect_id}/confirm", tag = "accounts", security(("oidc" = [])),
    params(("connect_id" = String, Path, description = "The ID returned by the credential check")),
    request_body = ConfirmRequest,
    responses((status = 200, body = ConfirmResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `sending_method_exists`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn confirm(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(connect_id): ApiPath<String>,
    ApiJson(body): ApiJson<ConfirmRequest>,
) -> Result<Json<ConfirmResponse>, ApiError> {
    let confirmed = state
        .accounts
        .confirm(&scope, &connect_id, body.account_ids, audit)
        .await?;
    Ok(Json(confirmed.into()))
}

/// Lists the accounts the caller reaches, newest first.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/accounts", operation_id = "list_accounts", tag = "accounts", security(("oidc" = [])),
    params(AccountListQuery),
    responses((status = 200, body = Page<AccountDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<AccountListQuery>,
) -> Result<Json<Page<AccountDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let platform = q.platform.as_deref().map(platform_id).transpose()?;
    let page = state
        .accounts
        .list(
            &scope,
            platform.as_ref(),
            q.include_disconnected.unwrap_or(false),
            pagination,
        )
        .await?;
    Ok(Json(Page::new(
        page.data.into_iter().map(AccountDto::from).collect(),
        page.total,
        &pagination,
    )))
}

/// One account.
///
/// # Errors
///
/// 404 for a missing account or one outside a delegate's account limit.
#[utoipa::path(get, path = "/api/v1/accounts/{id}", operation_id = "get_account", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<AccountDto>, ApiError> {
    Ok(Json(
        state
            .accounts
            .get(&scope, AccountId::from(id))
            .await?
            .into(),
    ))
}

/// Changes the test flag or display name. Last writer wins: accounts carry no version.
///
/// # Errors
///
/// 422 for a blank or over-long name.
#[utoipa::path(patch, path = "/api/v1/accounts/{id}", operation_id = "update_account", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    request_body = UpdateAccountRequest,
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn update(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<UpdateAccountRequest>,
) -> Result<Json<AccountDto>, ApiError> {
    let view = state
        .accounts
        .update(
            &scope,
            AccountId::from(id),
            body.test_account,
            body.display_name,
        )
        .await?;
    Ok(Json(view.into()))
}

/// Asks the platform whether the account still works. A rejected credential is not an error:
/// the response shows the account as `needs_reconnect`.
///
/// # Errors
///
/// 422 `validation_failed` for a disconnected account; 503 when the platform cannot answer.
#[utoipa::path(post, path = "/api/v1/accounts/{id}/check", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (the account is disconnected)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn check(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<AccountDto>, ApiError> {
    let view = state
        .accounts
        .check(&scope, AccountId::from(id), audit)
        .await?;
    Ok(Json(view.into()))
}

/// Disconnects an account: cancels its pending and parked deliveries and keeps the row for
/// history.
///
/// # Errors
///
/// 409 `job_active` while a post is being published to the account.
#[utoipa::path(delete, path = "/api/v1/accounts/{id}", operation_id = "disconnect_account", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = DisconnectResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`job_active`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn disconnect(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<DisconnectResponse>, ApiError> {
    let outcome = state
        .accounts
        .disconnect(&scope, AccountId::from(id), audit)
        .await?;
    Ok(Json(outcome.into()))
}

/// Imports credential accounts from a CSV file (`Content-Type: text/csv`). The header row
/// lists the platform's credential fields and may add `display_name`. With `commit=false`
/// (the default) it only checks every row; with `commit=true` it connects the valid ones. The
/// report never contains a field value.
///
/// # Errors
///
/// 413 past the configured size, 422 for a bad header or a file over the row limit.
#[utoipa::path(post, path = "/api/v1/accounts/import/{platform}", tag = "accounts", security(("oidc" = [])),
    params(("platform" = String, Path, description = "Platform ID"), ImportQuery),
    request_body(content = String, content_type = "text/csv", description = "One credential per row"),
    responses((status = 200, body = ImportReportDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `owner_only` or `capability_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn import(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(platform): ApiPath<String>,
    ApiQuery(q): ApiQuery<ImportQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<ImportReportDto>, ApiError> {
    let is_csv = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("text/csv"));
    if !is_csv {
        return Err(ApiError::new(ErrorCode::ValidationFailed)
            .with_detail("expected Content-Type: text/csv"));
    }
    charge(&state, &scope)?;
    let platform = platform_id(&platform)?;

    let stream = body.into_data_stream().map_err(io::Error::other);
    let too_large = Arc::new(AtomicBool::new(false));
    let reader = LimitedReader {
        inner: StreamReader::new(Box::pin(stream)),
        remaining: state.settings.csv_max_bytes,
        too_large: Arc::clone(&too_large),
    };
    let result = state
        .accounts
        .import_csv(&scope, &platform, reader, q.commit.unwrap_or(false), audit)
        .await;
    if too_large.load(Ordering::Relaxed) {
        return Err(ApiError::new(ErrorCode::PayloadTooLarge));
    }
    Ok(Json(result?.into()))
}

/// Counts the bytes read and fails past the limit, remembering that it did: the CSV reader
/// reports any read failure as a malformed file, so the handler checks the flag.
struct LimitedReader<R> {
    inner: R,
    remaining: usize,
    too_large: Arc<AtomicBool>,
}

impl<R: AsyncRead + Unpin> AsyncRead for LimitedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = buf.filled().len() - before;
                if read > self.remaining {
                    self.too_large.store(true, Ordering::Relaxed);
                    return Poll::Ready(Err(io::Error::other("body too large")));
                }
                self.remaining -= read;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                // The body-limit layer's own error: the body was over the limit.
                if error.to_string().contains("length limit exceeded")
                    || std::error::Error::source(&error)
                        .is_some_and(|s| s.to_string().contains("length limit exceeded"))
                {
                    self.too_large.store(true, Ordering::Relaxed);
                }
                Poll::Ready(Err(error))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
