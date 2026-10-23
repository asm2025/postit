use std::net::IpAddr;
use std::sync::Arc;

use postit_config::HttpSettings;
use tracing::Instrument as _;
use url::Host;
use uuid::Uuid;

use crate::error::HttpError;
use crate::policy::{AddressPolicy, PolicyResolver, is_allowed};
use crate::redact::{Sensitivity, redacted_error_text, redactor};

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

/// The **public client** for user-supplied hosts, from [`build_public_client`]. It carries
/// its [`AddressPolicy`], so every send refuses forbidden IP literals without the caller
/// passing the policy again: [`execute_public`], or [`crate::send_with_retry`] through
/// [`ClientRef::Public`]. It deliberately does not deref to `reqwest::Client`: that client's
/// own `execute` and request builders would skip the literal check. Build requests with
/// `reqwest::Request::new`.
#[derive(Clone, Debug)]
pub struct PublicClient {
    client: reqwest::Client,
    policy: AddressPolicy,
}

impl PublicClient {
    /// The address policy this client enforces.
    #[must_use]
    pub fn policy(&self) -> AddressPolicy {
        self.policy
    }
}

/// The client a send goes through: a trusted client from [`build_client`], or a
/// [`PublicClient`], whose IP-literal check then runs before every attempt.
#[derive(Clone, Copy, Debug)]
pub enum ClientRef<'a> {
    /// A client for operator-configured hosts.
    Trusted(&'a reqwest::Client),
    /// A client for user-supplied hosts.
    Public(&'a PublicClient),
}

impl<'a> From<&'a reqwest::Client> for ClientRef<'a> {
    fn from(client: &'a reqwest::Client) -> Self {
        Self::Trusted(client)
    }
}

impl<'a> From<&'a PublicClient> for ClientRef<'a> {
    fn from(client: &'a PublicClient) -> Self {
        Self::Public(client)
    }
}

impl<'a> ClientRef<'a> {
    /// The `reqwest` client to send `request` with, after a public client's IP-literal check.
    pub(crate) fn checked(
        self,
        request: &reqwest::Request,
    ) -> Result<&'a reqwest::Client, HttpError> {
        match self {
            Self::Trusted(client) => Ok(client),
            Self::Public(public) => {
                check_literal(request, public.policy)?;
                Ok(&public.client)
            }
        }
    }
}

/// Builds the **public client** for user-supplied hosts. Same base as [`build_client`],
/// plus: a resolver that drops every address `policy` forbids, no redirects (a 3xx is
/// returned to the caller), and no proxy, so `HTTP_PROXY`/`HTTPS_PROXY` cannot route around
/// the resolver. reqwest skips the resolver for IP-literal hosts, so the returned
/// [`PublicClient`] keeps `policy` and checks literals on every send.
///
/// # Errors
///
/// Returns [`HttpError::Permanent`] when an extra CA file can't be read or parsed, or when
/// the underlying `reqwest` client fails to build.
pub fn build_public_client(
    settings: &HttpSettings,
    policy: AddressPolicy,
) -> Result<PublicClient, HttpError> {
    // `.no_proxy()` has no observable getter on the built client, and a test that sets
    // `HTTP_PROXY` would need `unsafe` `set_var` (forbidden here), so the builder chain below
    // is the assertion.
    let client = finish(
        base_builder(settings)?
            .dns_resolver(Arc::new(PolicyResolver::new(policy)))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy(),
    )?;
    Ok(PublicClient { client, policy })
}

/// Sends a request through a [`PublicClient`]: refuses a URL whose host is an IP literal
/// the client's policy forbids before anything is sent (the resolver never sees literals),
/// then sends it as [`execute_traced`] does. The check uses the parsed host, so obfuscated
/// literals (`http://2130706433/`, `http://[::ffff:7f00:1]/`) are caught after `url`
/// normalizes them.
///
/// # Errors
///
/// Returns [`HttpError::AddressRejected`] when the host is a forbidden IP literal or the
/// resolver refused every address, and otherwise what [`execute_traced`] returns.
pub async fn execute_public(
    client: &PublicClient,
    request: reqwest::Request,
) -> Result<reqwest::Response, HttpError> {
    let client = ClientRef::Public(client).checked(&request)?;
    execute_traced(client, request).await
}

/// Refuses a request whose host is an IP literal that `policy` forbids. The resolver never
/// sees literals, so every public-client send runs this first.
fn check_literal(request: &reqwest::Request, policy: AddressPolicy) -> Result<(), HttpError> {
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

/// Executes a request in its own tracing span (request id, method, and the url rendered
/// through the process-wide [`crate::redactor`]), and
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
    send_traced(client, request, None, Sensitivity::Normal)
        .await
        .map_err(HttpError::from)
}

/// The span-wrapped send behind [`execute_traced`], returning the raw `reqwest` error so the
/// retry loop can tell a connect failure from a failure after the request went out. Records
/// `attempt` on the span when the retry loop passes one. Under [`Sensitivity::Opaque`] the
/// span shows `[redacted-url]` and the returned error carries no URL at all.
pub(crate) async fn send_traced(
    client: &reqwest::Client,
    request: reqwest::Request,
    attempt: Option<u32>,
    sensitivity: Sensitivity,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_id = new_request_id();
    let method = request.method().clone();
    let redacted_url = redactor().render_url(request.url(), sensitivity);

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
        let mut result = client.execute(request).await;
        if sensitivity == Sensitivity::Opaque {
            result = result.map_err(reqwest::Error::without_url);
        }
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

/// The trusted client as an agent HTTP handle. `reqwest::Client` is foreign, so the handle
/// is a newtype that `postit-agent-kit` downcasts.
#[derive(Clone, Debug)]
pub struct StandardClient(pub reqwest::Client);

impl postit_core::HttpHandle for StandardClient {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl postit_core::HttpHandle for PublicClient {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The two clients an agent may use: the trusted `standard` client for the platform's own
/// hosts, and the `public` client for hosts the user supplied.
#[must_use]
pub fn agent_handles(standard: reqwest::Client, public: PublicClient) -> postit_core::HttpHandles {
    postit_core::HttpHandles {
        default: Arc::new(StandardClient(standard)),
        user_hosts: Arc::new(public),
    }
}
