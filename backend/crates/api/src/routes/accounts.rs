//! Connected accounts: pasted-credential and OAuth connection, listing, checks, and
//! disconnecting.

use std::collections::BTreeMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use futures::TryStreamExt as _;
use postit_core::{
    AccountId, AgentError, DirectRecipient, DirectSendOutcome, DirectSendResult, MediaAsset,
    MediaAssetState, PlatformId, PostKind, Schedule, TEST_SEND_ID, ValidationInput, Variant,
    is_dkim_selector, validate_target,
};
use postit_data::app_settings::AppSettingsRepo;
use postit_data::users::UsersRepo;
use postit_data::{Capability, DataError, OwnerScope};
use postit_oauth::AccountError;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::io::{AsyncRead, ReadBuf};
use tokio_util::io::StreamReader;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_accounts::{
    AccountDto, ConfirmRequest, ConfirmResponse, CredentialCheckResponse, DisconnectResponse,
    DnsCheckResponse, ImportReportDto, OAuthStartResponse, PendingConnectionDto, StartOAuthRequest,
    TestEmailRequest, UpdateAccountRequest,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{RequestAudit, Scope};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

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
    responses((status = 200, body = CredentialCheckResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `violations`) or `credentials_invalid` (see `field`, `missing_scopes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn credentials(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(platform): ApiPath<String>,
    ApiJson(values): ApiJson<Map<String, Value>>,
) -> Result<Json<CredentialCheckResponse>, ApiError> {
    // The bucket of requests that call a platform on the user's behalf.
    state.limits.credential_checks.charge(&scope.actor)?;
    let check = state
        .accounts
        .start_credentials(&scope, &platform_id(&platform)?, values)
        .await?;
    Ok(Json(check.into()))
}

/// Starts connecting accounts of an OAuth platform. Send the user's browser to
/// `authorize_url`; the platform redirects it to the callback, which ends on `return_to` with
/// `result=ok&connect_id=…` (read the accounts with `GET /accounts/connect/{id}`, then confirm)
/// or `result=error&code=…`. Owner only.
///
/// # Errors
///
/// 422 `return_to_invalid` unless `return_to` is exactly an allowed return URL, 409
/// `capability_unsupported` for a credentials platform, 404 for an unknown platform, 503 when
/// the platform is not configured here.
#[utoipa::path(post, path = "/api/v1/accounts/connect/{id}", operation_id = "start_oauth_connection", tag = "accounts", security(("oidc" = [])),
    params(("id" = String, Path, description = "Platform ID, such as `mock_oauth`")),
    request_body = StartOAuthRequest,
    responses((status = 200, body = OAuthStartResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (an owner-only action through `X-Postit-Act-As`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`return_to_invalid` or `validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn start_oauth(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(platform): ApiPath<String>,
    ApiJson(body): ApiJson<StartOAuthRequest>,
) -> Result<Json<OAuthStartResponse>, ApiError> {
    scope.require(postit_data::Capability::OwnerOnly)?;
    state.limits.credential_checks.charge(&scope.actor)?;
    let start = state
        .accounts
        .start_oauth(&scope, &platform_id(&platform)?, &body.return_to)
        .await?;
    Ok(Json(start.into()))
}

/// Reads a pending connection, from an OAuth sign-in or pasted credentials, without
/// consuming it: the accounts to offer before confirming. Only the user who started it may
/// read it; anyone else's read ends it and answers 404.
///
/// # Errors
///
/// 404 for an unknown, expired, used, or foreign `connect_id`.
#[utoipa::path(get, path = "/api/v1/accounts/connect/{id}", operation_id = "get_pending_connection", tag = "accounts", security(("oidc" = [])),
    params(("id" = String, Path, description = "The `connect_id` from the OAuth callback's redirect or the credential check")),
    responses((status = 200, body = PendingConnectionDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (an owner-only action through `X-Postit-Act-As`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn pending(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(connect_id): ApiPath<String>,
) -> Result<Json<PendingConnectionDto>, ApiError> {
    Ok(Json(
        state.accounts.pending(&scope, &connect_id).await?.into(),
    ))
}

/// Connects the accounts the user picked from a checked credential or an OAuth sign-in. The
/// `connect_id` works once, for the user who started the connection.
///
/// # Errors
///
/// 404 for an unknown, expired, used, or foreign `connect_id`; 409 `platform_disabled` or
/// `sending_method_exists`.
#[utoipa::path(post, path = "/api/v1/accounts/connect/{connect_id}/confirm", tag = "accounts", security(("oidc" = [])),
    params(("connect_id" = String, Path, description = "The ID returned by the credential check or the OAuth callback's redirect")),
    request_body = ConfirmRequest,
    responses((status = 200, body = ConfirmResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `sending_method_exists`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
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
    responses((status = 200, body = Page<AccountDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
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
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
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

/// The options the account's platform accepts on a post, as the agent describes them, with any
/// consent text. Needs `edit`; may call the platform, so it spends the credential-check bucket.
///
/// # Errors
///
/// 403 `capability_required` below `edit`; 404 for a missing account or one outside a
/// delegate's account limit; 429 when the credential-check limit is spent.
#[utoipa::path(get, path = "/api/v1/accounts/{id}/publish-options", operation_id = "get_publish_options", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 200, body = postit_core::OptionsSchema),
        (status = 403, description = "`capability_required`, `account_disabled`, `account_pending`, or `delegation_invalid`", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"),
        crate::responses::CommonProblems))]
pub async fn publish_options(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<postit_core::OptionsSchema>, ApiError> {
    scope.require(Capability::Edit)?;
    state.limits.credential_checks.charge(&scope.actor)?;
    Ok(Json(
        state
            .accounts
            .publish_options(&scope, AccountId::from(id))
            .await?,
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
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
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
    responses((status = 200, body = AccountDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (the account is disconnected)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
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
    responses((status = 200, body = DisconnectResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`job_active`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
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

/// How long the test email may take: the default request timeout is shorter than one slow
/// SMTP exchange, and the connect and each command may each take `email.command_timeout`.
pub const TEST_EMAIL_TIMEOUT: Duration = Duration::from_secs(120);

/// Sends `email` once to the signed-in user's own verified address through a direct-send
/// account, such as an email sending method. A test send writes no delivery row, counts
/// against no daily cap, tracks nothing, and carries no unsubscribe headers; its footer link
/// shows a "this was a test" page. Owner only. Unsaved drafts are fine: the document is the
/// body, not a post.
///
/// # Errors
///
/// 409 `live_publish_disabled` while live publishing is off, `platform_disabled`,
/// `capability_unsupported` for an account that cannot send to people,
/// `email_address_missing` when the user has no verified address, and `test_send_refused`
/// when the mail server did not take the mail for the address, refusing it or failing for
/// now (the agent's reason code in `detail`); 422
/// `validation_failed` with every rule the document breaks; 404 for a missing account or
/// image. A failed send answers with the mapped agent error, its code in `detail`.
#[utoipa::path(post, path = "/api/v1/accounts/{id}/test-email", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID")),
    request_body = TestEmailRequest,
    responses((status = 204, description = "Sent"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (an owner-only action through `X-Postit-Act-As`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 408, description = "`request_timeout`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`live_publish_disabled`, `platform_disabled`, `capability_unsupported`, `email_address_missing` or `test_send_refused` (the mail server refused the address or failed for now; the reason code in `detail`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `violations`) or `credentials_invalid` (the account's credential was refused)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn test_email(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<TestEmailRequest>,
) -> Result<StatusCode, ApiError> {
    scope.require(Capability::OwnerOnly)?;
    // The bucket shared by the requests that reach an email account's server or DNS.
    state.limits.email_checks.charge(&scope.actor)?;
    let address = {
        let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
        let live = match AppSettingsRepo::live_publishing(&mut conn).await {
            Ok(live) => live,
            // Never seeded: off, as the worker reads it.
            Err(DataError::NotFound) => false,
            Err(other) => return Err(other.into()),
        };
        if !live {
            return Err(ApiError::new(ErrorCode::LivePublishDisabled));
        }
        UsersRepo::find_by_id(&mut conn, scope.actor)
            .await?
            .filter(|user| user.email_verified)
            .and_then(|user| user.email)
            .filter(|email| !email.trim().is_empty())
    };
    // The connection above is released before the platform is called.
    let bound = state
        .accounts
        .bound_client(scope.owner, AccountId::from(id))
        .await?;
    let Some(sender) = bound.client.direct() else {
        return Err(ApiError::new(ErrorCode::CapabilityUnsupported)
            .with_detail("this account cannot send to people"));
    };
    let Some(address) = address else {
        return Err(ApiError::new(ErrorCode::EmailAddressMissing));
    };

    let language = body.email.language;
    let variant = Variant {
        target: bound.account.clone(),
        kind: PostKind::Post,
        text: String::new(),
        media: Vec::new(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: Some(body.email),
        options: Map::new(),
    };
    let assets = test_email_assets(&state, &scope, &variant).await?;
    let descriptor = &bound.descriptor;
    let mut violations = validate_target(&ValidationInput {
        descriptor,
        options_schema: &descriptor.options_schema,
        variant: &variant,
        media: &assets,
        schedule: Schedule::Now,
        now: state.clock.now(),
    });
    match bound.client.validate(&variant, &assets).await {
        Ok(found) | Err(AgentError::Validation(found)) => violations.extend(found),
        Err(error) => return Err(agent_failure(error)),
    }
    if !violations.is_empty() {
        return Err(AccountError::Validation(violations).into());
    }

    let recipient = DirectRecipient {
        send_id: TEST_SEND_ID,
        address,
        merge_fields: BTreeMap::new(),
        locale: language.as_str().to_owned(),
    };
    let results = sender
        .send_batch(
            &bound.account,
            std::slice::from_ref(&recipient),
            &variant,
            &assets,
        )
        .await
        .map_err(agent_failure)?;
    // `send_batch` answers `Ok` for a refused recipient; the result says what happened.
    match results.into_iter().find(|r| r.send_id == TEST_SEND_ID) {
        Some(DirectSendResult {
            outcome: DirectSendOutcome::Sent { .. },
            ..
        }) => Ok(StatusCode::NO_CONTENT),
        Some(DirectSendResult {
            outcome:
                DirectSendOutcome::Bounced { reason, .. } | DirectSendOutcome::Failed { reason, .. },
            ..
        }) => Err(ApiError::new(ErrorCode::TestSendRefused).with_detail(reason)),
        None => Err(ApiError::internal(
            &"the agent returned no result for the test send",
        )),
    }
}

/// The stored media the test email's image blocks embed, all ready, or 404.
async fn test_email_assets(
    state: &AppState,
    scope: &OwnerScope,
    variant: &Variant,
) -> Result<Vec<MediaAsset>, ApiError> {
    let ids = variant.media_ids();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let assets = state.media.assets(scope, &ids).await?;
    if assets
        .iter()
        .any(|asset| asset.state != MediaAssetState::Ready)
    {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    Ok(assets)
}

/// Query of the DNS check.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DnsCheckQuery {
    /// The DKIM selector to check, one DNS label such as `s1` or `google` (letters, digits,
    /// and inner hyphens, at most 63); the sending service names it. Without one, DKIM is
    /// `unknown`.
    pub dkim_selector: Option<String>,
}

/// Looks up the SPF, DKIM, and DMARC records of the domain an account sends from and says,
/// per record, whether it is `ok`, `missing`, `weak`, or `unknown` (the lookup failed, or no
/// DKIM selector was given), with a fixed `code` the clients translate: `spf_ok`,
/// `spf_missing`, `spf_weak`, `spf_unknown`, `dkim_ok`, `dkim_missing`, `dkim_unknown`,
/// `dmarc_ok`, `dmarc_missing`, `dmarc_weak`, `dmarc_unknown`. Nothing is sent. Owner only,
/// limited by the same bucket as the test email.
///
/// # Errors
///
/// 409 `capability_unsupported` for an account that cannot check a sending domain, or
/// `platform_disabled`; 422 `validation_failed` (`field` is `dkim_selector`) for a selector
/// that is not one DNS label; 404 for a missing account.
#[utoipa::path(get, path = "/api/v1/accounts/{id}/dns-check", tag = "accounts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Account ID"), DnsCheckQuery),
    responses((status = 200, body = DnsCheckResponse), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (an owner-only action through `X-Postit-Act-As`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed` (see `field`) or `credentials_invalid` (the account's credential was refused)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn dns_check(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<DnsCheckQuery>,
) -> Result<Json<DnsCheckResponse>, ApiError> {
    scope.require(Capability::OwnerOnly)?;
    let selector = q.dkim_selector.as_deref();
    if selector.is_some_and(|s| !is_dkim_selector(s)) {
        return Err(
            ApiError::new(ErrorCode::ValidationFailed).with_extension("field", "dkim_selector")
        );
    }
    // The bucket shared by the requests that reach an email account's server or DNS.
    state.limits.email_checks.charge(&scope.actor)?;
    let bound = state
        .accounts
        .bound_client(scope.owner, AccountId::from(id))
        .await?;
    let Some(check) = bound.client.domain_check() else {
        return Err(ApiError::new(ErrorCode::CapabilityUnsupported)
            .with_detail("this account cannot check a sending domain"));
    };
    let findings = check
        .check(&bound.account, selector)
        .await
        .map_err(agent_failure)?;
    Ok(Json(DnsCheckResponse { findings }))
}

/// A failed agent call as the API answers it, with the agent's fixed error code in `detail`.
fn agent_failure(error: AgentError) -> ApiError {
    let code = error.code();
    match error {
        AgentError::Validation(violations) => AccountError::Validation(violations).into(),
        AgentError::Unsupported { .. } => {
            ApiError::new(ErrorCode::CapabilityUnsupported).with_detail(code)
        }
        other => ApiError::from(AccountError::from_agent(other)).with_detail(code),
    }
}

/// Imports credential accounts from a CSV file (`Content-Type: text/csv`). The header row
/// lists the platform's credential fields and may add `display_name`. With `commit=false`
/// (the default) it only checks every row; with `commit=true` it connects the valid ones. The
/// report never contains a field value.
///
/// # Errors
///
/// 413 past the configured size, 422 for a bad header. A file over the row limit is read up to
/// the limit and the report says `truncated`.
#[utoipa::path(post, path = "/api/v1/accounts/import/{platform}", tag = "accounts", security(("oidc" = [])),
    params(("platform" = String, Path, description = "Platform ID"), ImportQuery),
    request_body(content = String, content_type = "text/csv", description = "One credential per row"),
    responses((status = 200, body = ImportReportDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (an owner-only action through `X-Postit-Act-As`) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`platform_disabled` or `capability_unsupported`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
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
    // The bucket of requests that call a platform on the user's behalf.
    state.limits.credential_checks.charge(&scope.actor)?;
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
