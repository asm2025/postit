use postit_config::HttpSettings;
use tracing::Instrument as _;
use uuid::Uuid;

use crate::error::HttpError;
use crate::redact::redact_query_params;

/// Builds the workspace's only `reqwest::Client`: rustls with the platform's native root
/// store, plus any extra CA files from config, connection pooling, timeouts, a fixed user
/// agent, and gzip. No global `Content-Type` and no cookie store.
///
/// # Errors
///
/// Returns [`HttpError::Permanent`] when an extra CA file can't be read or parsed, or when
/// the underlying `reqwest` client fails to build.
pub fn build_client(settings: &HttpSettings) -> Result<reqwest::Client, HttpError> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(settings.connect_timeout)
        .timeout(settings.request_timeout)
        .user_agent(settings.user_agent.clone())
        .gzip(true);

    for ca_file in &settings.extra_ca_files {
        let pem = std::fs::read(ca_file).map_err(|err| {
            HttpError::Permanent(format!("reading CA file {}: {err}", ca_file.display()))
        })?;
        let cert = reqwest::Certificate::from_pem(&pem).map_err(|err| {
            HttpError::Permanent(format!("parsing CA file {}: {err}", ca_file.display()))
        })?;
        builder = builder.add_root_certificate(cert);
    }

    builder
        .build()
        .map_err(|err| HttpError::Permanent(format!("building http client: {err}")))
}

/// A request id for tracing spans and the `X-Request-Id` header. `postit-api` generates
/// one per inbound request; outbound calls through `postit-http` get their own.
#[must_use]
pub fn new_request_id() -> Uuid {
    Uuid::now_v7()
}

/// Attaches `X-Request-Id` to a request builder and returns the id alongside it, so the
/// caller can log the same id it sent.
pub fn with_request_id(builder: reqwest::RequestBuilder) -> (reqwest::RequestBuilder, Uuid) {
    let id = new_request_id();
    (builder.header("X-Request-Id", id.to_string()), id)
}

/// Query parameter names that are never safe to log, redacted before a request's URL is
/// attached to its tracing span. Plan 03 phase B1 extends this from plugin-supplied patterns
/// (for example signed media URLs); this foundation list covers the common cases.
const SENSITIVE_QUERY_PARAMS: &[&str] = &["signature", "sig", "token", "access_token", "code"];

/// Executes a request in its own tracing span (request id, method, and a redacted url), and
/// classifies the outcome through [`HttpError`]. This is the one place `postit-http` sends a
/// request, so every outbound call gets the same span shape and the same redaction.
///
/// # Errors
///
/// Returns [`HttpError`] classified from the underlying `reqwest` failure; see
/// [`HttpError::from`].
pub async fn execute_traced(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> Result<reqwest::Response, HttpError> {
    let request_id = new_request_id();
    let method = request.method().clone();
    let redacted_url = redact_query_params(request.url(), SENSITIVE_QUERY_PARAMS);

    let span = tracing::info_span!(
        "http_request",
        %request_id,
        %method,
        url = %redacted_url,
        status = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );

    async move {
        let result = client.execute(request).await;
        match &result {
            Ok(response) => {
                tracing::Span::current().record("status", response.status().as_u16());
                tracing::Span::current().record("outcome", "ok");
            }
            Err(err) => {
                tracing::Span::current().record("outcome", "error");
                tracing::warn!(error = %err, "http request failed");
            }
        }
        result.map_err(HttpError::from)
    }
    .instrument(span)
    .await
}
