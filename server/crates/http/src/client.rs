use std::net::IpAddr;
use std::sync::Arc;

use postit_config::HttpSettings;
use tracing::Instrument as _;
use url::Host;
use uuid::Uuid;

use crate::error::HttpError;
use crate::policy::{AddressPolicy, PolicyResolver, is_allowed};
use crate::redact::{SENSITIVE_QUERY_PARAMS, redact_query_params, redacted_error_text};

/// The builder setup shared by both clients: rustls with the platform's native root store,
/// plus any extra CA files from config, connection pooling, timeouts, a fixed user agent,
/// and gzip. No global `Content-Type` and no cookie store.
fn base_builder(settings: &HttpSettings) -> Result<reqwest::ClientBuilder, HttpError> {
    #[allow(clippy::disallowed_methods)] // postit-http is the only client factory
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

    Ok(builder)
}

fn finish(builder: reqwest::ClientBuilder) -> Result<reqwest::Client, HttpError> {
    builder
        .build()
        .map_err(|err| HttpError::Permanent(format!("building http client: {err}")))
}

/// Builds the **trusted client** for operator-configured hosts (the OIDC issuer, platform
/// APIs with fixed hosts): rustls with the platform's native root store, plus any extra CA
/// files from config, connection pooling, timeouts, a fixed user agent, and gzip. It honors
/// proxy environment variables and follows redirects.
///
/// # Errors
///
/// Returns [`HttpError::Permanent`] when an extra CA file can't be read or parsed, or when
/// the underlying `reqwest` client fails to build.
pub fn build_client(settings: &HttpSettings) -> Result<reqwest::Client, HttpError> {
    finish(base_builder(settings)?)
}

/// Builds the **public client** for user-supplied hosts. Same base as [`build_client`],
/// plus: a resolver that drops every address `policy` forbids, no redirects (a 3xx is
/// returned to the caller), and no proxy, so `HTTP_PROXY`/`HTTPS_PROXY` cannot route around
/// the resolver. reqwest skips the resolver for IP-literal hosts, so send every request
/// through [`execute_public`].
///
/// # Errors
///
/// Returns [`HttpError::Permanent`] when an extra CA file can't be read or parsed, or when
/// the underlying `reqwest` client fails to build.
pub fn build_public_client(
    settings: &HttpSettings,
    policy: AddressPolicy,
) -> Result<reqwest::Client, HttpError> {
    // `.no_proxy()` has no observable getter on the built client, and a test that sets
    // `HTTP_PROXY` would need `unsafe` `set_var` (forbidden here), so the builder chain below
    // is the assertion.
    finish(
        base_builder(settings)?
            .dns_resolver(Arc::new(PolicyResolver::new(policy)))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy(),
    )
}

/// Sends a request through a client from [`build_public_client`]: refuses a URL whose host
/// is an IP literal that `policy` forbids before anything is sent (the resolver never sees
/// literals), then calls [`execute_traced`]. The check uses the parsed host, so obfuscated
/// literals (`http://2130706433/`, `http://[::ffff:7f00:1]/`) are caught after `url`
/// normalizes them.
///
/// # Errors
///
/// Returns [`HttpError::AddressRejected`] when the host is a forbidden IP literal or the
/// resolver refused every address, and otherwise what [`execute_traced`] returns.
pub async fn execute_public(
    client: &reqwest::Client,
    request: reqwest::Request,
    policy: AddressPolicy,
) -> Result<reqwest::Response, HttpError> {
    check_literal(&request, policy)?;
    execute_traced(client, request).await
}

/// Refuses a request whose host is an IP literal that `policy` forbids. The resolver never
/// sees literals, so every public-client send runs this first.
pub(crate) fn check_literal(
    request: &reqwest::Request,
    policy: AddressPolicy,
) -> Result<(), HttpError> {
    let literal = match request.url().host() {
        Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        Some(Host::Domain(_)) | None => None,
    };
    if let Some(ip) = literal
        && !is_allowed(ip, policy)
    {
        return Err(HttpError::AddressRejected {
            host: ip.to_string(),
        });
    }
    Ok(())
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
    send_traced(client, request, None)
        .await
        .map_err(HttpError::from)
}

/// The span-wrapped send behind [`execute_traced`], returning the raw `reqwest` error so the
/// retry loop can tell a connect failure from a failure after the request went out. Records
/// `attempt` on the span when the retry loop passes one.
pub(crate) async fn send_traced(
    client: &reqwest::Client,
    request: reqwest::Request,
    attempt: Option<u32>,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_id = new_request_id();
    let method = request.method().clone();
    let redacted_url = redact_query_params(request.url(), SENSITIVE_QUERY_PARAMS);

    let span = tracing::info_span!(
        "http_request",
        %request_id,
        %method,
        url = %redacted_url,
        attempt = tracing::field::Empty,
        status = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    if let Some(attempt) = attempt {
        span.record("attempt", attempt);
    }

    async move {
        let result = client.execute(request).await;
        match &result {
            Ok(response) => {
                tracing::Span::current().record("status", response.status().as_u16());
                tracing::Span::current().record("outcome", "ok");
            }
            Err(err) => {
                tracing::Span::current().record("outcome", "error");
                tracing::warn!(error = %redacted_error_text(err), "http request failed");
            }
        }
        result
    }
    .instrument(span)
    .await
}
