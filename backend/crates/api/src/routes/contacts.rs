//! Contacts, contact lists, the suppression list, and CSV import and export (plan 03 C1).
//!
//! Reads need `view` and changes `edit`; importing, exporting, reading an import, and removing
//! a suppression are owner-only (`owner_only` under `X-Postit-Act-As`). The service checks each
//! one. Import and export stream, so they live in the custom-limits group
//! ([`super::v1_custom_limits_router`]) with their own per-user rate limit,
//! `rate_limit.contact_transfers`.

use std::io;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::extract::multipart::{Multipart, MultipartError, MultipartRejection};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::TryStreamExt as _;
use postit_contacts::ImportOptions;
use postit_core::{ContactId, ContactImportId, ContactListId, SuppressionId};
use postit_data::OwnerScope;
use postit_data::contacts::ContactFilter;
use serde::Deserialize;
use tokio_util::io::StreamReader;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto_contacts::{
    ContactDto, ContactImportDto, ContactImportForm, ContactListDto, ContactListRequest,
    ContactStatusDto, CreateContactRequest, SuppressionDto, UpdateContactRequest,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::Scope;
use crate::idempotency::{IdempotencyKey, StoredResponse, request_hash, run};
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// The import route, as the idempotency key records it.
const IMPORT_ROUTE: &str = "/api/v1/contacts/import";
/// The largest `options` part read.
const OPTIONS_MAX: usize = 64 * 1024;

/// One charge per import or export request against `rate_limit.contact_transfers`.
fn charge(state: &AppState, scope: &OwnerScope) -> Result<(), ApiError> {
    state
        .limits
        .contact_transfers
        .check(&scope.actor)
        .map_err(|wait| ApiError::new(ErrorCode::RateLimited).with_retry_after(wait))
}

fn invalid(detail: &'static str) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed).with_detail(detail)
}

fn json_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal(&e))
}

/// A response with a `Location` built from the body's `id`.
fn with_location(mut response: Response, path: &str, body: &serde_json::Value) -> Response {
    let location = body
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| HeaderValue::from_str(&format!("{path}/{id}")).ok());
    if let Some(location) = location {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ContactListQuery {
    /// Only members of this list.
    pub list_id: Option<Uuid>,
    /// Only contacts with this tag.
    pub tag: Option<String>,
    /// Only contacts with this status.
    pub status: Option<ContactStatusDto>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// Which contacts to export.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ExportQuery {
    /// Only members of this list.
    pub list_id: Option<Uuid>,
}

/// The owner's contacts, oldest first, narrowed by list, tag, or status.
///
/// # Errors
///
/// 422 for invalid paging or filters.
#[utoipa::path(get, path = "/api/v1/contacts", operation_id = "list_contacts", tag = "contacts", security(("oidc" = [])),
    params(ContactListQuery),
    responses((status = 200, body = Page<ContactDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, or `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<ContactListQuery>,
) -> Result<Json<Page<ContactDto>>, ApiError> {
    let pagination = PageQuery {
        page: q.page,
        page_size: q.page_size,
    }
    .to_pagination()?;
    let filter = ContactFilter {
        list_id: q.list_id.map(ContactListId::from_uuid),
        tag: q.tag,
        status: q.status.map(Into::into),
    };
    let set = state.contacts.list(&scope, pagination, filter).await?;
    Ok(Json(Page::new(
        set.data.into_iter().map(ContactDto::from).collect(),
        set.total,
        &pagination,
    )))
}

/// Adds a contact with its consent record, or updates the contact that has the address.
/// A new contact counts against the owner's contacts cap; an update never does. Needs `edit`.
///
/// # Errors
///
/// 409 `contact_limit_exceeded` at the cap; 422 `validation_failed` with `field` (`email` or
/// `consent_source`).
#[utoipa::path(post, path = "/api/v1/contacts", operation_id = "create_contact", tag = "contacts", security(("oidc" = [])),
    request_body = CreateContactRequest,
    responses((status = 201, body = ContactDto, headers(("Location" = String, description = "`/api/v1/contacts/{id}`"))), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`contact_limit_exceeded`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` names `email` or `consent_source`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<CreateContactRequest>,
) -> Result<Response, ApiError> {
    let contact = state.contacts.create(&scope, body.into()).await?;
    let body = json_value(&ContactDto::from(contact))?;
    let response = (StatusCode::CREATED, Json(body.clone())).into_response();
    Ok(with_location(response, "/api/v1/contacts", &body))
}

/// One contact.
///
/// # Errors
///
/// 404 when the owner has no such contact.
#[utoipa::path(get, path = "/api/v1/contacts/{id}", operation_id = "get_contact", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Contact ID")),
    responses((status = 200, body = ContactDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, or `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<ContactDto>, ApiError> {
    let contact = state.contacts.get(&scope, ContactId::from_uuid(id)).await?;
    Ok(Json(contact.into()))
}

/// Changes a contact. The address never changes (an `email` member is refused). `status`
/// moves between `subscribed` and `unsubscribed` only; unsubscribing keeps the address
/// suppressed even if the contact is deleted and added again, and only the owner can
/// subscribe it again. Needs `edit`.
///
/// # Errors
///
/// 403 `owner_only` when a delegate subscribes a contact the owner unsubscribed; 404 for an
/// unknown contact; 409 `status_not_settable` (`bounced` or `complained`, or the address
/// bounced or complained) or `suppressed` (subscribing an address that bounced, complained,
/// or opted out); 422 `validation_failed`.
#[utoipa::path(patch, path = "/api/v1/contacts/{id}", operation_id = "update_contact", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Contact ID")),
    request_body = UpdateContactRequest,
    responses((status = 200, body = ContactDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to), `owner_only` (subscribing a contact the owner unsubscribed) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`status_not_settable` or `suppressed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` is `consent_source` for a blank one", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn update(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<UpdateContactRequest>,
) -> Result<Json<ContactDto>, ApiError> {
    let patch = body.into_patch()?;
    let contact = state
        .contacts
        .update(&scope, ContactId::from_uuid(id), patch)
        .await?;
    Ok(Json(contact.into()))
}

/// Deletes a contact. Its suppression entry, if any, stays. Needs `edit`.
///
/// # Errors
///
/// 404 when the owner has no such contact.
#[utoipa::path(delete, path = "/api/v1/contacts/{id}", operation_id = "delete_contact", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Contact ID")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .contacts
        .delete(&scope, ContactId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The owner's contact lists, with their member counts.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/contact-lists", operation_id = "list_contact_lists", tag = "contacts", security(("oidc" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<ContactListDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, or `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn lists(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> Result<Json<Page<ContactListDto>>, ApiError> {
    let pagination = q.to_pagination()?;
    let set = state.contacts.lists(&scope, pagination).await?;
    Ok(Json(Page::new(
        set.data.into_iter().map(ContactListDto::from).collect(),
        set.total,
        &pagination,
    )))
}

/// Creates an empty list. Needs `edit`.
///
/// # Errors
///
/// 422 `validation_failed` with `field` `name` for a blank name.
#[utoipa::path(post, path = "/api/v1/contact-lists", operation_id = "create_contact_list", tag = "contacts", security(("oidc" = [])),
    request_body = ContactListRequest,
    responses((status = 201, body = ContactListDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` is `name`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create_list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<ContactListRequest>,
) -> Result<(StatusCode, Json<ContactListDto>), ApiError> {
    let list = state.contacts.create_list(&scope, &body.name).await?;
    Ok((StatusCode::CREATED, Json(list.into())))
}

/// Renames a list. Needs `edit`.
///
/// # Errors
///
/// 404 for an unknown list; 422 `validation_failed` with `field` `name` for a blank name.
#[utoipa::path(patch, path = "/api/v1/contact-lists/{id}", operation_id = "rename_contact_list", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "List ID")),
    request_body = ContactListRequest,
    responses((status = 200, body = ContactListDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; `field` is `name`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn rename_list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ContactListRequest>,
) -> Result<Json<ContactListDto>, ApiError> {
    let list = state
        .contacts
        .rename_list(&scope, ContactListId::from_uuid(id), &body.name)
        .await?;
    Ok(Json(list.into()))
}

/// Deletes a list; its contacts stay. Recorded as `contact_list_deleted`. Needs `edit`.
///
/// # Errors
///
/// 404 for an unknown list.
#[utoipa::path(delete, path = "/api/v1/contact-lists/{id}", operation_id = "delete_contact_list", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "List ID")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_list(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .contacts
        .delete_list(&scope, ContactListId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Adds a contact to a list; one already in it stays. Needs `edit`.
///
/// # Errors
///
/// 404 when the list or the contact is not the owner's.
#[utoipa::path(put, path = "/api/v1/contact-lists/{id}/members/{contact_id}", operation_id = "add_contact_list_member", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "List ID"), ("contact_id" = Uuid, Path, description = "Contact ID")),
    responses((status = 204, description = "The contact is in the list"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn add_member(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath((id, contact)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .contacts
        .add_member(
            &scope,
            ContactListId::from_uuid(id),
            &[ContactId::from_uuid(contact)],
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a contact from a list; one not in it changes nothing. Needs `edit`.
///
/// # Errors
///
/// 404 for an unknown list.
#[utoipa::path(delete, path = "/api/v1/contact-lists/{id}/members/{contact_id}", operation_id = "remove_contact_list_member", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "List ID"), ("contact_id" = Uuid, Path, description = "Contact ID")),
    responses((status = 204, description = "The contact is not in the list"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn remove_member(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath((id, contact)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .contacts
        .remove_member(
            &scope,
            ContactListId::from_uuid(id),
            &[ContactId::from_uuid(contact)],
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn part_error(err: &MultipartError) -> ApiError {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(ErrorCode::PayloadTooLarge)
    } else {
        invalid("the multipart body could not be read")
    }
}

/// Reads the whole `options` part, refusing one over [`OPTIONS_MAX`].
async fn read_options(multipart: &mut Multipart) -> Result<(ImportOptions, Vec<u8>), ApiError> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| part_error(&e))?
        .ok_or_else(|| invalid("the options part is required"))?;
    if field.name() != Some("options") {
        return Err(invalid("the options part must come before the file"));
    }
    let mut raw = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|e| part_error(&e))? {
        if raw.len() + chunk.len() > OPTIONS_MAX {
            return Err(invalid("the options part is too large"));
        }
        raw.extend_from_slice(&chunk);
    }
    let options = serde_json::from_slice(&raw)
        .map_err(|_| invalid("the options part is not valid import options"))?;
    Ok((options, raw))
}

/// Starts a CSV import. The body is `multipart/form-data` with an `options` part (JSON) and
/// then a `file` part. `options.attest_consent` must be `true`; `columns` maps CSV headers to
/// contact fields (`email` and `consent_source` required). The file is read as it arrives:
/// rows the file condemns are reported at once, the rest are applied by a background job, and
/// `GET /contacts/imports/{id}` shows progress. A replay with the same `Idempotency-Key`,
/// `options`, and `Content-Length` answers the first import. Owner-only.
///
/// # Errors
///
/// 400 `consent_attestation_required`; 404 for an unknown `list_id`; 413 `csv_too_large`
/// past `contacts.csv_max_bytes` or `contacts.csv_max_rows`, or `payload_too_large` (with
/// `max_bytes`) for a declared length far beyond them; 422 `csv_invalid` (a mapped header
/// missing, or not UTF-8) or `validation_failed` (not multipart, parts out of order, bad
/// options).
#[utoipa::path(post, path = "/api/v1/contacts/import", operation_id = "import_contacts", tag = "contacts", security(("oidc" = [])),
    request_body(content = ContactImportForm, content_type = "multipart/form-data"),
    params(("Idempotency-Key" = Option<String>, Header, description = "Replays the first response for the same options and length")),
    responses((status = 202, body = ContactImportDto, headers(("Location" = String, description = "`/api/v1/contacts/imports/{id}`"))), (status = 400, description = "`consent_attestation_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner imports)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found` (`list_id`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 408, description = "`request_timeout`: the body sat idle too long", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`idempotency_in_progress`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`csv_too_large`, or `payload_too_large` (see `max_bytes`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`csv_invalid`, `validation_failed`, or `idempotency_key_reused`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn import(
    State(state): State<AppState>,
    Scope(scope): Scope,
    IdempotencyKey(key): IdempotencyKey,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, ApiError> {
    charge(&state, &scope)?;
    let mut multipart = multipart.map_err(|_| invalid("expected multipart/form-data"))?;
    let (options, raw) = read_options(&mut multipart).await?;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let mut input = raw;
    input.push(b'\n');
    input.extend_from_slice(declared.as_bytes());
    let hash = request_hash(&Method::POST, IMPORT_ROUTE, &input);
    let (state_ref, scope_ref) = (&state, &scope);
    let stored = run(
        &state.pool,
        state.clock.as_ref(),
        &scope,
        key.as_deref(),
        IMPORT_ROUTE,
        &hash,
        move || async move {
            let field = multipart
                .next_field()
                .await
                .map_err(|e| part_error(&e))?
                .ok_or_else(|| invalid("the file part is required"))?;
            if field.name() != Some("file") {
                return Err(invalid("the second part must be the file"));
            }
            let reader = StreamReader::new(Box::pin(field.map_err(io::Error::other)));
            let import = state_ref
                .contacts
                .start_import(scope_ref, options, reader)
                .await?;
            Ok(StoredResponse {
                status: StatusCode::ACCEPTED,
                body: json_value(&ContactImportDto::from(import))?,
            })
        },
    )
    .await?;
    let body = stored.body.clone();
    Ok(with_location(
        stored.into_response(),
        "/api/v1/contacts/imports",
        &body,
    ))
}

/// One import, with its live counts and the rows it rejected. Owner-only.
///
/// # Errors
///
/// 404 when the owner has no such import.
#[utoipa::path(get, path = "/api/v1/contacts/imports/{id}", operation_id = "get_contact_import", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Import ID")),
    responses((status = 200, body = ContactImportDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner imports)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_import(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<ContactImportDto>, ApiError> {
    let import = state
        .contacts
        .get_import(&scope, ContactImportId::from_uuid(id))
        .await?;
    Ok(Json(import.into()))
}

/// The owner's contacts as a CSV download, optionally only one list's members, streamed. The
/// columns are `email`, `first_name`, `last_name`, `tags` (joined with `|`), `locale`,
/// `consent_source`, `consent_at`, `status`, then one per custom field. A cell a spreadsheet
/// would run as a formula starts with `'`; the import removes it. Recorded as
/// `contacts_exported` before the first byte. Owner-only.
///
/// # Errors
///
/// 404 for an unknown `list_id`.
#[utoipa::path(get, path = "/api/v1/contacts/export", operation_id = "export_contacts", tag = "contacts", security(("oidc" = [])),
    params(ExportQuery),
    responses((status = 200, description = "The contacts as CSV", content_type = "text/csv", body = String), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner exports)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found` (`list_id`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn export(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<ExportQuery>,
) -> Result<Response, ApiError> {
    charge(&state, &scope)?;
    let stream = state
        .contacts
        .export(&scope, q.list_id.map(ContactListId::from_uuid))
        .await?;
    let body = Body::from_stream(stream.map_err(|err| {
        // The download is cut short; the error names no contact.
        tracing::warn!(error = %err, "contact export stopped");
        io::Error::other("the export stopped")
    }));
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"contacts.csv\"",
            ),
        ],
        body,
    )
        .into_response())
}

/// The owner's suppression list: addresses that are never emailed. An entry shows its
/// contact's ID and address while a contact with that address exists.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/suppressions", operation_id = "list_suppressions", tag = "contacts", security(("oidc" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<SuppressionDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, or `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn suppressions(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> Result<Json<Page<SuppressionDto>>, ApiError> {
    let pagination = q.to_pagination()?;
    let set = state.contacts.suppressions(&scope, pagination).await?;
    Ok(Json(Page::new(
        set.data.into_iter().map(SuppressionDto::from).collect(),
        set.total,
        &pagination,
    )))
}

/// Removes a bounce from the suppression list, so the address can be emailed again; its
/// contact goes back to `subscribed` unless the owner had unsubscribed it. An opt-out or a
/// complaint can only be reversed by the contact. Recorded as `suppression_removed`.
/// Owner-only.
///
/// # Errors
///
/// 404 for an unknown entry; 409 `suppression_not_removable` unless it is a bounce.
#[utoipa::path(delete, path = "/api/v1/suppressions/{id}", operation_id = "delete_suppression", tag = "contacts", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Suppression entry ID")),
    responses((status = 204, description = "Removed"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner removes a suppression)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`suppression_not_removable`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn remove_suppression(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .contacts
        .remove_suppression(&scope, SuppressionId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
