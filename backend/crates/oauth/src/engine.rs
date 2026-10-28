//! The provider-neutral OAuth 2.0 token client: the authorize URL, the code exchange, refresh,
//! and RFC 7009 revocation, as form posts through the `postit-http` standard client.
//!
//! Nothing here logs or returns a secret: tokens, codes, the PKCE verifier, `state`, client
//! secrets, and token response bodies stay out of logs, errors, and `Debug`. A failed token
//! request keeps only the OAuth `error` code.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use emixcrypto::HashAlgorithm as _;
use postit_core::{
    ClientAuth, HttpHandles, OAuthProviderSpec, OAuthTokens, PlatformId, RawTokenResponse,
    TokenExchange,
};
use postit_http::{
    HttpError, Idempotency, Jitter, RateLimitHeaders, RetryPolicy, RetryRequest, Sensitivity,
    StandardClient, send_with_retry,
};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::{Map, Value};
use url::Url;

use crate::AccountError;

/// The longest OAuth `error` code kept from a response; anything longer is not one.
const MAX_ERROR_CODE_LEN: usize = 64;

/// A PKCE verifier and its S256 challenge.
#[derive(Debug)]
pub struct Pkce {
    /// The secret sent with the code exchange.
    pub verifier: SecretString,
    /// `base64url(SHA-256(verifier))`, sent on the authorize URL.
    pub challenge: String,
}

impl Pkce {
    /// A fresh verifier of 32 random bytes, base64url without padding, and its challenge.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::fill(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let digest = emixcrypto::Sha256Hash::new()
            .compute_hash_bytes(verifier.as_bytes())
            .unwrap_or_default();
        Self {
            challenge: URL_SAFE_NO_PAD.encode(digest),
            verifier: SecretString::from(verifier),
        }
    }
}

/// The deployment's client ID and secret for one platform.
#[derive(Debug, Clone)]
pub struct ClientCredentials {
    /// The client ID.
    pub id: String,
    /// The client secret.
    pub secret: SecretString,
}

impl ClientCredentials {
    /// Reads `client_id` and `client_secret` from the platform's `[platforms.<id>]` table.
    ///
    /// # Errors
    ///
    /// [`AccountError::PlatformUnavailable`] when either is missing, empty, or not a string.
    pub fn from_config(config: &Value) -> Result<Self, AccountError> {
        let field = |name: &str| {
            config
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        };
        match (field("client_id"), field("client_secret")) {
            (Some(id), Some(secret)) => Ok(Self {
                id: id.to_owned(),
                secret: SecretString::from(secret.to_owned()),
            }),
            _ => Err(AccountError::PlatformUnavailable),
        }
    }
}

/// Why a token, refresh, or revoke request failed. Carries no part of the response body
/// beyond a validated OAuth `error` code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// `invalid_grant`: the code or refresh token is no longer valid.
    #[error("invalid grant")]
    InvalidGrant,
    /// `invalid_client` or `unauthorized_client`: the deployment's client config is wrong.
    #[error("invalid client")]
    InvalidClient,
    /// Any other OAuth error, by its code only. `server_error` and
    /// `temporarily_unavailable` (the codes a 5xx carries) are retryable.
    #[error("provider error: {0}")]
    Provider(String),
    /// The request failed in transit, the provider answered 5xx without an OAuth code, or the
    /// outcome is unknown (a non-idempotent request that may have been acted on).
    #[error("transport failure")]
    Transport,
}

impl TokenError {
    /// Whether trying again later could succeed.
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::Transport => true,
            Self::Provider(code) => is_server_code(code),
            Self::InvalidGrant | Self::InvalidClient => false,
        }
    }

    /// A short name for logs.
    fn name(&self) -> &str {
        match self {
            Self::InvalidGrant => "invalid_grant",
            Self::InvalidClient => "invalid_client",
            Self::Provider(code) => code,
            Self::Transport => "transport",
        }
    }
}

fn is_server_code(code: &str) -> bool {
    matches!(code, "server_error" | "temporarily_unavailable")
}

/// The callback URL for `platform`: `{public_url}/api/v1/platforms/{id}/oauth/callback`,
/// joined onto a base whose path ends in `/` so no double or dropped segment appears.
#[must_use]
pub fn redirect_uri(public_url: &Url, platform: &PlatformId) -> Url {
    let mut base = public_url.clone();
    base.set_query(None);
    base.set_fragment(None);
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    let relative = format!("api/v1/platforms/{}/oauth/callback", platform.as_str());
    // A relative path of `[a-z0-9_/]` joins onto any URL that has a path.
    base.join(&relative).unwrap_or(base)
}

/// The URL the user is sent to for approval.
#[must_use]
pub fn authorize_url(
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    redirect_uri: &Url,
    state: &str,
    pkce: Option<&Pkce>,
) -> Url {
    let mut url = spec.authorize_url.clone();
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair(&spec.client_id_param, &client.id)
            .append_pair("redirect_uri", redirect_uri.as_str())
            .append_pair("scope", &spec.scopes.join(&spec.scope_separator))
            .append_pair("state", state);
        if let Some(pkce) = pkce {
            query
                .append_pair("code_challenge", &pkce.challenge)
                .append_pair("code_challenge_method", "S256");
        }
        for (name, value) in &spec.extra_authorize_params {
            query.append_pair(name, value);
        }
    }
    url
}

/// Exchanges an authorization code. Sent once: a code is single use, so a resend after a
/// lost response could only fail.
///
/// # Errors
///
/// A [`TokenError`] classified from the OAuth `error` code; an unknown outcome is
/// [`TokenError::Transport`].
pub async fn exchange_code(
    http: &HttpHandles,
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    redirect_uri: &Url,
    code: &SecretString,
    verifier: Option<&SecretString>,
    now: DateTime<Utc>,
) -> Result<TokenExchange, TokenError> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.expose_secret()),
        ("redirect_uri", redirect_uri.as_str()),
    ];
    if let Some(verifier) = verifier {
        form.push(("code_verifier", verifier.expose_secret()));
    }
    let body = token_form(spec, client, &form, true);
    let map = post_form(
        http,
        spec,
        client,
        &spec.token_url,
        body,
        Idempotency::NonIdempotent,
        now,
    )
    .await?;
    parse_tokens(spec, map, now)
}

/// Exchanges a refresh token for new tokens. A response without `refresh_token` carries
/// none: the caller keeps the old one. Resent after an ambiguous failure only when the
/// provider does not rotate refresh tokens.
///
/// # Errors
///
/// A [`TokenError`] classified from the OAuth `error` code.
pub async fn refresh(
    http: &HttpHandles,
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    refresh_token: &SecretString,
    now: DateTime<Utc>,
) -> Result<TokenExchange, TokenError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.expose_secret()),
    ];
    let body = token_form(spec, client, &form, true);
    let idempotency = if spec.refresh.rotates {
        Idempotency::NonIdempotent
    } else {
        Idempotency::Idempotent
    };
    let map = post_form(http, spec, client, &spec.token_url, body, idempotency, now).await?;
    parse_tokens(spec, map, now)
}

/// Revokes `token` at the spec's RFC 7009 endpoint, with `token_type_hint` set to `hint`.
///
/// # Errors
///
/// [`TokenError::Provider`] with `revocation_unsupported` when the spec has no revocation
/// endpoint (nothing is sent); otherwise a [`TokenError`] classified as for the token
/// endpoint.
pub async fn revoke(
    http: &HttpHandles,
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    token: &SecretString,
    hint: &str,
) -> Result<(), TokenError> {
    let Some(revoke_url) = &spec.revoke_url else {
        return Err(TokenError::Provider("revocation_unsupported".to_owned()));
    };
    let form = [("token", token.expose_secret()), ("token_type_hint", hint)];
    let body = token_form(spec, client, &form, false);
    let response = send(
        http,
        client,
        spec.client_auth,
        revoke_url,
        body,
        Idempotency::Idempotent,
        Utc::now(),
    )
    .await?;
    drop(response);
    Ok(())
}

/// Scopes the spec asks for that `granted` lacks. Empty when `granted` is empty: a response
/// without `scope` granted what was asked.
#[must_use]
pub fn missing_scopes(spec: &OAuthProviderSpec, granted: &[String]) -> Vec<String> {
    if granted.is_empty() {
        return Vec::new();
    }
    spec.scopes
        .iter()
        .filter(|scope| !granted.contains(scope))
        .cloned()
        .collect()
}

/// The form body: the request's own fields, the client ID (and secret for `PostBody`), then
/// the spec's extra token parameters when `extras`.
fn token_form(
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    fields: &[(&str, &str)],
    extras: bool,
) -> String {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.extend_pairs(fields);
    match spec.client_auth {
        ClientAuth::Basic => {}
        ClientAuth::PostBody => {
            form.append_pair(&spec.client_id_param, &client.id);
            form.append_pair("client_secret", client.secret.expose_secret());
        }
        ClientAuth::None => {
            form.append_pair(&spec.client_id_param, &client.id);
        }
    }
    if extras {
        for (name, value) in &spec.extra_token_params {
            form.append_pair(name, value);
        }
    }
    form.finish()
}

/// Posts to the token endpoint and reads the JSON object it answers with.
async fn post_form(
    http: &HttpHandles,
    spec: &OAuthProviderSpec,
    client: &ClientCredentials,
    url: &Url,
    body: String,
    idempotency: Idempotency,
    now: DateTime<Utc>,
) -> Result<Map<String, Value>, TokenError> {
    let response = send(http, client, spec.client_auth, url, body, idempotency, now).await?;
    let Ok(Value::Object(map)) = response.json::<Value>().await else {
        return Err(logged(TokenError::Provider("invalid_response".to_owned())));
    };
    // Some providers answer 200 with an OAuth error body.
    if map.contains_key("error") {
        return Err(logged(classify(200, error_code(&map).as_deref())));
    }
    Ok(map)
}

/// Sends one form post with the client's authentication, under `postit-http`'s retry rules.
async fn send(
    http: &HttpHandles,
    client: &ClientCredentials,
    client_auth: ClientAuth,
    url: &Url,
    body: String,
    idempotency: Idempotency,
    now: DateTime<Utc>,
) -> Result<reqwest::Response, TokenError> {
    let Some(StandardClient(standard)) = http.default.as_any().downcast_ref::<StandardClient>()
    else {
        tracing::error!("the OAuth engine was given the wrong kind of HTTP client");
        return Err(TokenError::Transport);
    };
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let request = RetryRequest {
        client: standard.into(),
        policy: &policy,
        idempotency,
        headers: &headers,
        now: Arc::new(move || now),
        limiter: None,
        sensitivity: Sensitivity::Normal,
    };
    let make = || {
        let mut builder = standard
            .post(url.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body.clone());
        if client_auth == ClientAuth::Basic {
            builder = builder.basic_auth(&client.id, Some(client.secret.expose_secret()));
        }
        builder.build().map_err(HttpError::from)
    };
    let response = send_with_retry(request, &mut Jitter::from_entropy(), make)
        .await
        .map_err(|error| logged(from_http(&error)))?;
    // `postit-http` hands back a redirect it did not follow; it is no answer to a form post.
    let status = response.status();
    if !status.is_success() {
        return Err(logged(TokenError::Provider(format!(
            "http_{}",
            status.as_u16()
        ))));
    }
    Ok(response)
}

/// Logs the failure by its code only.
fn logged(error: TokenError) -> TokenError {
    tracing::warn!(error = error.name(), "oauth token request failed");
    error
}

fn from_http(error: &HttpError) -> TokenError {
    match error {
        HttpError::Status {
            status,
            body_excerpt,
            ..
        } => classify(
            *status,
            body_excerpt
                .as_deref()
                .and_then(excerpt_error_code)
                .as_deref(),
        ),
        HttpError::Permanent(_) => TokenError::Provider("request_failed".to_owned()),
        HttpError::Transient(_)
        | HttpError::RateLimited { .. }
        | HttpError::OutcomeUnknown(_)
        | HttpError::AddressRejected { .. } => TokenError::Transport,
    }
}

/// The response's OAuth `error` code, when it looks like one (`[a-z_]`, bounded); a value
/// that does not could be anything, so it is never kept.
fn error_code(map: &Map<String, Value>) -> Option<String> {
    valid_code(map.get("error")?.as_str()?)
}

/// The `error` code from a failed response's body excerpt. `postit-http` redacts secret-looking
/// values in the excerpt (`"access_token": [redacted]`), which can leave it invalid JSON, so
/// this falls back to finding the `"error": "<code>"` member in the text.
fn excerpt_error_code(text: &str) -> Option<String> {
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) {
        return error_code(&map);
    }
    let after_key = text.split_once("\"error\"")?.1.trim_start();
    let value = after_key
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    valid_code(value.split_once('"')?.0)
}

fn valid_code(code: &str) -> Option<String> {
    let valid = !code.is_empty()
        && code.len() <= MAX_ERROR_CODE_LEN
        && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    valid.then(|| code.to_owned())
}

fn classify(status: u16, code: Option<&str>) -> TokenError {
    match code {
        Some("invalid_grant") => TokenError::InvalidGrant,
        Some("invalid_client" | "unauthorized_client") => TokenError::InvalidClient,
        None if status == 401 => TokenError::InvalidClient,
        Some(code) if status < 500 || is_server_code(code) => TokenError::Provider(code.to_owned()),
        // A 5xx with no usable code, or with a code that would read as permanent.
        _ if status >= 500 => TokenError::Transport,
        _ => TokenError::Provider(format!("http_{status}")),
    }
}

/// Parses a successful token response.
fn parse_tokens(
    spec: &OAuthProviderSpec,
    map: Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<TokenExchange, TokenError> {
    let string = |name: &str| {
        map.get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let Some(access_token) = string("access_token") else {
        return Err(logged(TokenError::Provider("invalid_response".to_owned())));
    };
    let access_token = SecretString::from(access_token.to_owned());
    let refresh_token = string("refresh_token").map(|token| SecretString::from(token.to_owned()));
    let expires_at = expires_in(&map)
        .and_then(|seconds| i64::try_from(seconds).ok())
        .map(chrono::Duration::seconds)
        .or_else(|| {
            spec.default_access_ttl
                .and_then(|ttl| chrono::Duration::from_std(ttl).ok())
        })
        .and_then(|ttl| now.checked_add_signed(ttl));
    let scopes = scopes(spec, map.get("scope"));
    Ok(TokenExchange {
        tokens: OAuthTokens {
            access_token,
            refresh_token,
            expires_at,
            scopes,
        },
        raw: RawTokenResponse::new(map),
    })
}

/// `expires_in` as a number or a numeric string.
fn expires_in(map: &Map<String, Value>) -> Option<u64> {
    match map.get("expires_in")? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// The granted scopes: a string split on spaces or the spec's separator, or a list.
fn scopes(spec: &OAuthProviderSpec, value: Option<&Value>) -> Vec<String> {
    let separator = spec.scope_separator.as_str();
    let split = |text: &str| -> Vec<String> {
        text.split(' ')
            .flat_map(|part| {
                if separator.is_empty() || separator == " " {
                    vec![part]
                } else {
                    part.split(separator).collect()
                }
            })
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect()
    };
    match value {
        Some(Value::String(text)) => split(text),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec(separator: &str) -> OAuthProviderSpec {
        OAuthProviderSpec::new(
            Url::parse("https://auth.example/authorize").unwrap(),
            Url::parse("https://auth.example/token").unwrap(),
        )
        .with_scope_separator(separator)
    }

    #[test]
    fn scopes_split_on_spaces_and_the_separator() {
        assert_eq!(
            scopes(&spec(","), Some(&json!("read,write post"))),
            vec!["read", "write", "post"]
        );
        assert_eq!(
            scopes(&spec(" "), Some(&json!(" read  write "))),
            vec!["read", "write"]
        );
        assert_eq!(
            scopes(&spec(" "), Some(&json!(["read", "write"]))),
            vec!["read", "write"]
        );
        assert!(scopes(&spec(" "), None).is_empty());
    }

    #[test]
    fn classification_keeps_codes_only() {
        assert_eq!(
            classify(400, Some("invalid_grant")),
            TokenError::InvalidGrant
        );
        assert_eq!(
            classify(400, Some("unauthorized_client")),
            TokenError::InvalidClient
        );
        assert_eq!(classify(401, None), TokenError::InvalidClient);
        assert_eq!(classify(503, None), TokenError::Transport);
        assert_eq!(
            classify(500, Some("internal_failure")),
            TokenError::Transport
        );
        assert_eq!(
            classify(503, Some("temporarily_unavailable")),
            TokenError::Provider("temporarily_unavailable".to_owned())
        );
        assert_eq!(
            classify(404, None),
            TokenError::Provider("http_404".to_owned())
        );
        let map = json!({ "error": "Bearer abc.def" });
        assert_eq!(error_code(map.as_object().unwrap()), None);
        // A redacted excerpt is no longer JSON; the code is still found.
        assert_eq!(
            excerpt_error_code(
                r#"{"access_token": [redacted], "error_description": "x", "error" : "invalid_scope"}"#
            )
            .as_deref(),
            Some("invalid_scope")
        );
        assert_eq!(excerpt_error_code(r#"{"error": "Tok.123"}"#), None);
        assert_eq!(excerpt_error_code("not json"), None);
    }

    #[test]
    fn pkce_verifiers_differ() {
        let a = Pkce::generate();
        let b = Pkce::generate();
        assert_ne!(a.verifier.expose_secret(), b.verifier.expose_secret());
        assert_eq!(a.challenge.len(), 43);
    }
}
