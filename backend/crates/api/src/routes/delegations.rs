//! Delegation: owners grant, change, revoke, and list access to their workspace; delegates
//! list and leave the grants they hold.
//!
//! The owner routes take [`Scope`] and the service refuses a delegated scope with
//! `owner_only` (no re-delegation); the delegate routes take [`ActiveUser`], which refuses
//! `X-Postit-Act-As` outright.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::{AccountId, DelegationId, UserId};
use postit_data::Capability;
use postit_data::users::UsersRepo;
use postit_identity::delegation::{
    AccountsInput, ChangeRequest, DelegationDetail, DelegationParty, GrantRequest,
};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_delegations::{
    ChangeDelegationRequest, DelegationDto, DelegationUserDto, GrantDelegationRequest,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::{ActiveUser, RequestAudit, Scope};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DelegationListQuery {
    /// Include ended grants (default false).
    pub include_ended: Option<bool>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// `id` as one side of a grant: current name and status.
async fn user(state: &AppState, id: UserId) -> Result<DelegationUserDto, ApiError> {
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let record = UsersRepo::find_by_id(&mut conn, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    Ok(DelegationUserDto::new(
        record.id,
        record.display_name,
        record.status,
    ))
}

fn dto(detail: &DelegationDetail) -> DelegationDto {
    let party =
        |p: &DelegationParty| DelegationUserDto::new(p.id, p.display_name.clone(), p.status);
    DelegationDto::new(
        &detail.delegation,
        party(&detail.owner),
        party(&detail.delegate),
    )
}

fn accounts(ids: Vec<Uuid>) -> AccountsInput {
    AccountsInput::Only(ids.into_iter().map(AccountId::from).collect())
}

fn invalid(field: &'static str) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed).with_extension("field", field)
}

/// The caller's grants, newest first: live ones, or every one with `include_ended`.
/// Owner-only: `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
///
/// # Errors
///
/// 403 `owner_only` through `X-Postit-Act-As`; 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/delegations", operation_id = "list_delegations", tag = "delegations", security(("oidc" = [])),
    params(DelegationListQuery),
    responses((status = 200, body = Page<DelegationDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages grants)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<DelegationListQuery>,
) -> Result<Json<Page<DelegationDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let set = state
        .delegations
        .list_granted(&scope, q.include_ended.unwrap_or(false), pagination)
        .await?;
    let owner = user(&state, scope.owner).await?;
    let data = set
        .data
        .into_iter()
        .map(|view| {
            let delegate = DelegationUserDto::new(
                view.delegation.delegate_id,
                view.other_display_name,
                view.other_status,
            );
            DelegationDto::new(&view.delegation, owner.clone(), delegate)
        })
        .collect();
    Ok(Json(Page::new(data, set.total, &pagination)))
}

/// Lets another user act in the caller's workspace. Without `account_ids` the grant reaches
/// every account. Owner-only: `X-Postit-Act-As` is accepted but every delegate is refused
/// with `owner_only`.
///
/// # Errors
///
/// 409 `delegation_exists` when the user already holds a live grant (change it instead); 422
/// `validation_failed` with `field` (`delegate_id`, `account_ids`, or `expires_at`).
#[utoipa::path(post, path = "/api/v1/delegations", operation_id = "grant_delegation", tag = "delegations", security(("oidc" = [])),
    request_body = GrantDelegationRequest,
    responses((status = 201, body = DelegationDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages grants)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`delegation_exists`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` names `delegate_id` (unknown, not active, or the caller), `account_ids` (not the caller's connected account), or `expires_at` (not in the future)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn grant(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiJson(body): ApiJson<GrantDelegationRequest>,
) -> Result<(StatusCode, Json<DelegationDto>), ApiError> {
    let request = GrantRequest {
        delegate_id: UserId::from(body.delegate_id),
        level: body.level.into(),
        accounts: body.account_ids.map_or(AccountsInput::All, accounts),
        expires_at: body.expires_at,
    };
    let detail = state.delegations.grant(&scope, request, audit).await?;
    Ok((StatusCode::CREATED, Json(dto(&detail))))
}

/// Changes the level, account limit, or expiry of one of the caller's live grants. Absent
/// fields stay as they are; `expires_at: null` clears the expiry. Owner-only:
/// `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
///
/// # Errors
///
/// 404 for an unknown, ended, or expired grant; 422 `validation_failed` with `field`
/// (`all_accounts` when false, `account_ids` when sent with `all_accounts` or not the caller's
/// connected account, `expires_at` when not in the future).
#[utoipa::path(patch, path = "/api/v1/delegations/{id}", operation_id = "change_delegation", tag = "delegations", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delegation ID")),
    request_body = ChangeDelegationRequest,
    responses((status = 200, body = DelegationDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages grants)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found` (unknown, ended, or expired)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` names `all_accounts`, `account_ids`, or `expires_at`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn change(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ChangeDelegationRequest>,
) -> Result<Json<DelegationDto>, ApiError> {
    // Owner-only comes first, so a delegate never learns which body checks would fail.
    scope.require(Capability::OwnerOnly)?;
    let accounts = match (body.all_accounts, body.account_ids) {
        (Some(false), _) => return Err(invalid("all_accounts")),
        (Some(true), Some(_)) => return Err(invalid("account_ids")),
        (Some(true), None) => Some(AccountsInput::All),
        (None, ids) => ids.map(accounts),
    };
    let request = ChangeRequest {
        level: body.level.map(Into::into),
        accounts,
        expires_at: body.expires_at,
    };
    let detail = state
        .delegations
        .change(&scope, DelegationId::from(id), request, audit)
        .await?;
    Ok(Json(dto(&detail)))
}

/// Revokes one of the caller's grants. Revoking an ended grant does nothing. Owner-only:
/// `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
///
/// # Errors
///
/// 404 when the caller granted no such delegation.
#[utoipa::path(delete, path = "/api/v1/delegations/{id}", operation_id = "revoke_delegation", tag = "delegations", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delegation ID")),
    responses((status = 204, description = "Revoked, or already ended"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages grants)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn revoke(
    State(state): State<AppState>,
    Scope(scope): Scope,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .delegations
        .revoke(&scope, DelegationId::from(id), audit)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The grants the caller holds in other users' workspaces that have not ended, newest first.
/// Does not accept `X-Postit-Act-As` (`owner_only`).
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/delegations/received", operation_id = "list_received_delegations", tag = "delegations", security(("oidc" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<DelegationDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_received(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> Result<Json<Page<DelegationDto>>, ApiError> {
    let pagination = q.to_pagination()?;
    let set = state
        .delegations
        .list_received(principal.user_id, pagination)
        .await?;
    let delegate = user(&state, principal.user_id).await?;
    let data = set
        .data
        .into_iter()
        .map(|view| {
            let owner = DelegationUserDto::new(
                view.delegation.owner_id,
                view.other_display_name,
                view.other_status,
            );
            DelegationDto::new(&view.delegation, owner, delegate.clone())
        })
        .collect();
    Ok(Json(Page::new(data, set.total, &pagination)))
}

/// Leaves a grant the caller holds. Leaving an ended grant does nothing. Does not accept
/// `X-Postit-Act-As` (`owner_only`).
///
/// # Errors
///
/// 404 when the caller holds no such delegation.
#[utoipa::path(delete, path = "/api/v1/delegations/received/{id}", operation_id = "leave_delegation", tag = "delegations", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Delegation ID")),
    responses((status = 204, description = "Left, or already ended"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending` or `owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`unavailable` (signing keys not loaded yet)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn leave(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    RequestAudit(audit): RequestAudit,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .delegations
        .leave(principal.user_id, DelegationId::from(id), audit)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
