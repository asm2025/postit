//! A scriptable OAuth provider on loopback for testing token exchange, refresh, and revocation.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

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

/// What an approved code is bound to.
struct Approval {
    challenge: Option<String>,
    method: String,
    redirect_uri: Option<String>,
}

struct State {
    rotates: bool,
    refresh_issued: bool,
    client: Option<(String, String)>,
    scopes: Option<Vec<String>>,
    delay: Option<Duration>,
    codes: HashMap<String, Approval>,
    used_codes: HashSet<String>,
    token_requests: usize,
    issued: u32,
    failures: VecDeque<OAuthFailure>,
    requests: Vec<Request>,
    revoked: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            rotates: false,
            refresh_issued: true,
            client: None,
            scopes: None,
            delay: None,
            codes: HashMap::new(),
            used_codes: HashSet::new(),
            token_requests: 0,
            issued: 0,
            failures: VecDeque::new(),
            requests: Vec::new(),
            revoked: Vec::new(),
        }
    }
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

/// The client ID and secret a request presents, from HTTP Basic or the form body.
fn presented_client(request: &Request) -> (Option<String>, Option<String>) {
    let basic = request
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v.trim()).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .and_then(|text| {
            text.split_once(':')
                .map(|(id, secret)| (id.to_owned(), secret.to_owned()))
        });
    if let Some((id, secret)) = basic {
        return (Some(id), Some(secret));
    }
    let fields = form(request);
    let get = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    (get("client_id"), get("client_secret"))
}

/// A 401 `invalid_client` when a client is set and the request does not present it.
fn check_client(state: &State, request: &Request) -> Option<ResponseTemplate> {
    let (id, secret) = state.client.as_ref()?;
    let (got_id, got_secret) = presented_client(request);
    (got_id.as_deref() != Some(id) || got_secret.as_deref() != Some(secret))
        .then(|| oauth_error(401, "invalid_client", None))
}

fn pkce_matches(approval: &Approval, verifier: Option<&str>) -> bool {
    let Some(challenge) = &approval.challenge else {
        return true;
    };
    let Some(verifier) = verifier else {
        return false;
    };
    if approval.method == "plain" {
        return verifier == challenge;
    }
    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(verifier.as_bytes())
        .unwrap_or_default();
    URL_SAFE_NO_PAD.encode(digest) == *challenge
}

struct TokenEndpoint(Shared);

impl Respond for TokenEndpoint {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = lock(&self.0);
        state.requests.push(request.clone());
        state.token_requests += 1;
        if let Some(rejection) = check_client(&state, request) {
            return rejection;
        }
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
        if grant == Some("authorization_code")
            && let Some(code) = field("code")
        {
            let first_use = state.used_codes.insert(code.to_owned());
            if let Some(approval) = state.codes.get(code) {
                let valid = first_use
                    && pkce_matches(approval, field("code_verifier"))
                    && approval
                        .redirect_uri
                        .as_deref()
                        .is_none_or(|bound| field("redirect_uri") == Some(bound));
                if !valid {
                    return oauth_error(400, "invalid_grant", None);
                }
            }
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
        if let Some(refresh) = refresh.filter(|_| state.refresh_issued) {
            body["refresh_token"] = json!(refresh);
        }
        if let Some(scopes) = &state.scopes {
            body["scope"] = json!(scopes.join(" "));
        }
        let template = ResponseTemplate::new(200).set_body_json(body);
        match state.delay {
            Some(delay) => template.set_delay(delay),
            None => template,
        }
    }
}

struct RevokeEndpoint(Shared);

impl Respond for RevokeEndpoint {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = lock(&self.0);
        state.requests.push(request.clone());
        if let Some(rejection) = check_client(&state, request) {
            return rejection;
        }
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

    /// Requires this client at the token and revoke endpoints (HTTP Basic or the body's
    /// `client_id` and `client_secret`); anything else gets 401 `invalid_client`. Unset, any
    /// client is accepted. A spec whose `client_id_param` is not `client_id` is tested against
    /// its own server, not through this.
    pub fn set_client(&self, client_id: &str, client_secret: &str) {
        lock(&self.state).client = Some((client_id.to_owned(), client_secret.to_owned()));
    }

    /// Records the user's approval of `authorize_url` and returns the one-time code, bound to
    /// the URL's `code_challenge`, `code_challenge_method`, and `redirect_uri`. The token
    /// endpoint then requires the matching `code_verifier` and `redirect_uri` and accepts the
    /// code once; a mismatch is 400 `invalid_grant`. Codes not issued here are accepted as
    /// before. The `state` is the caller's to echo back to its callback.
    #[must_use]
    pub fn approve(&self, authorize_url: &Url) -> String {
        let query = |name: &str| {
            authorize_url
                .query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
        };
        let approval = Approval {
            challenge: query("code_challenge"),
            method: query("code_challenge_method").unwrap_or_else(|| "plain".to_owned()),
            redirect_uri: query("redirect_uri"),
        };
        let mut state = lock(&self.state);
        let code = format!("code-{}", state.codes.len() + 1);
        state.codes.insert(code.clone(), approval);
        code
    }

    /// The scopes token responses report as `scope`; `None` (the default) omits the field.
    pub fn set_granted_scopes(&self, scopes: Option<Vec<String>>) {
        lock(&self.state).scopes = scopes;
    }

    /// Whether responses carry a `refresh_token` (default true).
    pub fn set_refresh_issued(&self, issued: bool) {
        lock(&self.state).refresh_issued = issued;
    }

    /// How many `/token` requests arrived, failures included.
    #[must_use]
    pub fn token_requests(&self) -> usize {
        lock(&self.state).token_requests
    }

    /// Makes the token endpoint answer successfully only after `delay`.
    pub fn set_delay(&self, delay: Option<Duration>) {
        lock(&self.state).delay = delay;
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
