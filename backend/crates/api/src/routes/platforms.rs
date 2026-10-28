//! The routes a platform calls, not the user: the OAuth callback the provider redirects the
//! browser to, and the events a platform sends. Neither carries a bearer token; the `state` and
//! the agent's signature check are what authorize them.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::InboundEvent;
use postit_oauth::CallbackParams;
use secrecy::SecretString;
use utoipa::IntoParams;

use crate::error::{ApiError, ProblemDetails};
use crate::json::ApiPath;
use crate::state::AppState;

/// The callback route's timeout: the service's own deadline for the provider calls plus room
/// for the database work around them, so the API's default request timeout never answers a
/// browser with a 408 problem after the `state` was consumed.
pub const CALLBACK_TIMEOUT: Duration =
    postit_oauth::CALLBACK_DEADLINE.saturating_add(Duration::from_secs(30));

/// The largest platform event body read, in bytes; a larger one is refused with 413.
pub const EVENT_BODY_LIMIT: usize = 64 * 1024;

/// The query the provider's redirect carries. Documentation only: the handler reads the raw
/// query, so an unexpected or malformed parameter still ends in a redirect.
#[derive(Debug, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallbackQuery {
    /// The authorization code.
    pub code: Option<String>,
    /// The `state` postit sent with the authorization request.
    pub state: Option<String>,
    /// The provider's OAuth error, such as `access_denied`.
    pub error: Option<String>,
    /// The provider's description of the error. Ignored.
    pub error_description: Option<String>,
}

/// The provider's redirect after the user signed in (or refused). Not called by clients: the
/// browser arrives here from the platform, without a bearer token, and `X-Postit-Act-As` is
/// ignored; the connection belongs to the user who started it.
///
/// Always answers 303. The `Location` is the `return_to` the connection was started with,
/// carrying `result=ok&connect_id=…` on success (read the accounts with
/// `GET /accounts/connect/{id}`, then confirm) or `result=error&code=…`, where `code` is one of
/// `access_denied` (the user refused), `provider_error` (the provider reported another error or
/// sent no code), `exchange_failed` (the code could not be exchanged for tokens),
/// `scope_missing` (the user did not grant every permission the platform needs),
/// `discovery_failed` (the accounts could not be read), `no_accounts` (the sign-in reaches no
/// account), or `platform_unavailable` (the platform was switched off or is misconfigured). An
/// unknown, used, expired, or foreign `state` (`state_invalid`) goes to the web app's
/// `/oauth-done`, since no trustworthy `return_to` exists. Accounts another owner already
/// holds are skipped at confirm and counted in its `skipped_elsewhere`.
#[utoipa::path(get, path = "/api/v1/platforms/{id}/oauth/callback", operation_id = "oauth_callback", tag = "accounts",
    params(("id" = String, Path, description = "Platform ID"), CallbackQuery),
    responses((status = 303, description = "Back to `return_to` (or `/oauth-done`) with `result` and `connect_id` or `code`; see the operation description for the codes", headers(("Location" = String, description = "Where the browser goes next"))), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn oauth_callback(
    State(state): State<AppState>,
    platform: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    // An unreadable platform ID matches no state: the service answers `state_invalid`.
    let platform = platform.map(|Path(id)| id).unwrap_or_default();
    let mut params = CallbackParams {
        code: None,
        state: None,
        error: None,
    };
    // The first of each parameter counts; anything else (`error_description`, `iss`, `scope`)
    // is ignored and never logged.
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "code" if params.code.is_none() => {
                params.code = Some(SecretString::from(value.into_owned()));
            }
            "state" if params.state.is_none() => params.state = Some(value.into_owned()),
            "error" if params.error.is_none() => params.error = Some(value.into_owned()),
            _ => {}
        }
    }
    let target = state.accounts.oauth_callback(&platform, params).await;
    match HeaderValue::try_from(String::from(target)) {
        Ok(location) => (StatusCode::SEE_OTHER, [(header::LOCATION, location)]).into_response(),
        Err(error) => ApiError::internal(&error).into_response(),
    }
}

/// An event the platform sends, such as a deauthorization. Not called by clients. The agent
/// verifies it (a signature over the raw body, typically) and its reply is returned as is.
/// A verified deauthorization revokes every grant of that remote user on the platform.
///
/// # Errors
///
/// 401 with no detail when the agent cannot verify the event, 404 for an unknown or unavailable
/// platform or an event kind it does not handle, 413 past 64 KiB, 422 for a verified event the
/// agent cannot read.
#[utoipa::path(post, path = "/api/v1/platforms/{id}/events/{kind}", operation_id = "platform_event", tag = "accounts",
    params(("id" = String, Path, description = "Platform ID"), ("kind" = String, Path, description = "The agent's name for the event, such as `deauthorize`")),
    request_body(content = String, content_type = "application/octet-stream", description = "The platform's event, unmodified, at most 64 KiB"),
    responses((status = 200, description = "The agent's reply, in the status, `Content-Type`, and body the platform expects"), (status = 401, description = "`unauthenticated`: the event failed verification", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`owner_only` (the request carried `X-Postit-Act-As`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 413, description = "`payload_too_large`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`: the event could not be read", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 503, description = "`platform_unavailable`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn platform_event(
    State(state): State<AppState>,
    ApiPath((platform, kind)): ApiPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let event = InboundEvent {
        path_params: BTreeMap::from([
            ("id".to_owned(), platform.clone()),
            ("kind".to_owned(), kind.clone()),
        ]),
        kind,
        headers: headers
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
            .collect(),
        raw_body: body,
    };
    let reply = state
        .accounts
        .handle_platform_event(&platform, event)
        .await?;
    let status = StatusCode::from_u16(reply.status)
        .map_err(|_| ApiError::internal(&"the agent replied with an invalid status"))?;
    let mut response = (status, Body::from(reply.body)).into_response();
    if let Some(content_type) = reply.content_type {
        let value = HeaderValue::try_from(content_type).map_err(|e| ApiError::internal(&e))?;
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    Ok(response)
}
