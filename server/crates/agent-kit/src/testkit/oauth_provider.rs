//! A scriptable OAuth provider on loopback for testing token exchange, refresh, and revocation.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use emixcrypto::HashAlgorithm as _;
use postit_core::OAuthProviderSpec;
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// A failure to serve once from the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthFailure {
    /// 400 `invalid_grant`: the code or refresh token is no longer valid.
    InvalidGrant,
    /// 400 `invalid_grant` with a description saying the refresh token has expired. The status
    /// and error code match [`OAuthFailure::InvalidGrant`]; only `error_description` differs,
    /// which lets a test tell an expired token from a revoked one.
    ExpiredRefreshToken,
    /// 500 `server_error`.
    ServerError,
}

#[derive(Default)]
struct State {
    rotates: bool,
    issued: u32,
    failures: VecDeque<OAuthFailure>,
    requests: Vec<Request>,
    revoked: Vec<String>,
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Shared) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap()
}

fn form(request: &Request) -> Vec<(String, String)> {
    url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect()
}

fn hash(token: &str) -> String {
    emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(token.as_bytes())
        .unwrap_or_default()
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn oauth_error(status: u16, error: &str, description: Option<&str>) -> ResponseTemplate {
    let body = description.map_or_else(
        || json!({ "error": error }),
        |d| json!({ "error": error, "error_description": d }),
    );
    ResponseTemplate::new(status).set_body_json(body)
}

struct TokenEndpoint(Shared);

impl Respond for TokenEndpoint {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = lock(&self.0);
        state.requests.push(request.clone());
        if let Some(failure) = state.failures.pop_front() {
            return match failure {
                OAuthFailure::InvalidGrant => oauth_error(400, "invalid_grant", None),
                OAuthFailure::ExpiredRefreshToken => {
                    oauth_error(400, "invalid_grant", Some("The refresh token has expired."))
                }
                OAuthFailure::ServerError => oauth_error(500, "server_error", None),
            };
        }
        let fields = form(request);
        let field = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let grant = field("grant_type");
        if !matches!(grant, Some("authorization_code" | "refresh_token")) {
            return oauth_error(400, "unsupported_grant_type", None);
        }
        state.issued += 1;
        let n = state.issued;
        let mut body = json!({
            "access_token": format!("access-{n}"),
            "token_type": "Bearer",
            "expires_in": 3600,
        });
        let refresh = if grant == Some("authorization_code") || state.rotates {
            Some(format!("refresh-{n}"))
        } else {
            field("refresh_token").map(str::to_owned)
        };
        if let Some(refresh) = refresh {
            body["refresh_token"] = json!(refresh);
        }
        ResponseTemplate::new(200).set_body_json(body)
    }
}

struct RevokeEndpoint(Shared);

impl Respond for RevokeEndpoint {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = lock(&self.0);
        state.requests.push(request.clone());
        if let Some((_, token)) = form(request).into_iter().find(|(k, _)| k == "token") {
            state.revoked.push(hash(&token));
        }
        ResponseTemplate::new(200)
    }
}

/// An OAuth server with `/authorize`, `/token`, and `/revoke`. Only the token and revoke
/// endpoints answer; `/authorize` exists as a URL for building the user's redirect. The revoke
/// endpoint always succeeds: queued [`OAuthFailure`]s apply to the token endpoint only.
pub struct MockOAuthProvider {
    server: MockServer,
    state: Shared,
}

impl MockOAuthProvider {
    /// Starts the provider with refresh-token rotation off.
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let state = Shared::default();
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(TokenEndpoint(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .respond_with(RevokeEndpoint(state.clone()))
            .mount(&server)
            .await;
        Self { server, state }
    }

    /// The server's root URL, ending in a slash.
    #[must_use]
    pub fn base_url(&self) -> Url {
        Url::parse(&format!("{}/", self.server.uri())).expect("wiremock serves a valid URL")
    }

    /// The provider spec pointing at this server, with scopes `read` and `write`. The spec is a
    /// snapshot: its `refresh.rotates` reflects the rotation setting at the time of the call, so
    /// call [`MockOAuthProvider::set_rotation`] first and take the spec afterwards.
    #[must_use]
    pub fn spec(&self) -> OAuthProviderSpec {
        let base = self.base_url();
        let join = |p: &str| base.join(p).expect("a fixed relative path");
        let mut spec = OAuthProviderSpec::new(join("authorize"), join("token"))
            .with_revoke_url(join("revoke"))
            .with_scopes(vec!["read".to_owned(), "write".to_owned()]);
        spec.refresh.rotates = lock(&self.state).rotates;
        spec
    }

    /// Whether a refresh issues a new refresh token (otherwise the presented one stays).
    pub fn set_rotation(&self, rotates: bool) {
        lock(&self.state).rotates = rotates;
    }

    /// Queues a failure for the next token request; later requests succeed.
    pub fn fail_next(&self, failure: OAuthFailure) {
        lock(&self.state).failures.push_back(failure);
    }

    /// Every token and revoke request received, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.state).requests.clone()
    }

    /// The lowercase hex SHA-256 of each revoked token, never the token itself.
    #[must_use]
    pub fn revoked(&self) -> Vec<String> {
        lock(&self.state).revoked.clone()
    }
}
