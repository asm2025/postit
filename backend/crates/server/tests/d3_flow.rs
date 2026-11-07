//! The D3 exit run: the whole product flow over HTTP against the real composition (API and
//! worker in one process), as the owner and then as a `publish` delegate. Mock agents, the mock
//! OAuth provider, a wiremock AI provider, and the in-process SMTP sink stand in for platforms;
//! nothing here publishes live.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

use postit_agent_email::testkit::{Script, Sink};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::MockAgent;
use postit_config::{Environment, Settings};
use postit_core::{Agent, PlatformId};
use postit_identity::testkit::TestIssuer;
use postit_server::{Role, RunningServer, StartOptions, compose, start_with_agents};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT_ID_WEB: &str = "postit-app";
const OAUTH: &str = "mock_oauth";
const CLIENT_ID: &str = "postit-app";
const CLIENT_SECRET: &str = "postit-app-secret-value";
const DONE: &str = "http://127.0.0.1:44315/oauth-done";
const FROM: &str = "news@bakery.example";
const BOUNDARY: &str = "d3-flow-boundary";

fn config_dir(issuer: &TestIssuer, media: &std::path::Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/default.toml"
    ))
    .unwrap();
    std::fs::write(dir.path().join("default.toml"), base).unwrap();
    let dev = format!(
        r#"[server]
public_url = "http://127.0.0.1"
[database]
url = "postgres://localhost:5432/unused"
[auth.oidc]
issuer = "{issuer}"
audiences = ["postit"]
[auth.bootstrap]
admin_email = "admin@postit.test"
[mail]
from_address = "noreply@postit.test"
[mail.smtp]
host = "127.0.0.1"
port = 2525
tls = "none"
[app]
public_url = "http://127.0.0.1:44315"
[vault.keys]
1 = "insecure-dev-only-vault-key-00001"
[media]
storage_root = '{media}'
signing_key = "d3-media-signing-key-0123456789abc"
[email]
signing_key = "d3-email-signing-key-0123456789abc"
allow_private_hosts = true
command_timeout = "5s"
[ai]
allow_private_endpoints = true
[platforms.{OAUTH}]
client_id = "{CLIENT_ID}"
client_secret = "{CLIENT_SECRET}"
[rate_limit.unauthenticated]
rate_per_minute = 100000
burst = 100000
[rate_limit.authenticated]
rate_per_minute = 100000
burst = 100000
"#,
        issuer = issuer.issuer_url().to_string().trim_end_matches('/'),
        media = media.display()
    );
    std::fs::write(dir.path().join("development.toml"), dev).unwrap();
    dir
}

fn settings(dir: &tempfile::TempDir) -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "d3-key"),
    ];
    postit_config::load_with(
        Environment::Development,
        dir.path(),
        vars.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .unwrap()
}

/// A response read whole.
struct Reply {
    status: StatusCode,
    body: Value,
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn send(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        act_as: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let mut request = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(owner) = act_as {
            request = request.header("x-postit-act-as", owner);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(json) = body {
            request = request
                .header("content-type", "application/json")
                .body(json.to_string());
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.bytes().await.unwrap();
        let mut body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut body {
            if let Some(etag) = etag {
                map.insert("__etag".to_owned(), json!(etag));
            }
            if let Some(location) = location {
                map.insert("__location".to_owned(), json!(location));
            }
        } else if body.is_null() {
            let mut map = serde_json::Map::new();
            if let Some(location) = location {
                map.insert("__location".to_owned(), json!(location));
            }
            body = Value::Object(map);
        }
        Reply { status, body }
    }

    /// A request as `who`: their token, acting in `owner`'s workspace when given.
    async fn call(
        &self,
        who: &User,
        owner: Option<&User>,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Reply {
        let act_as = owner.map(|o| o.id.clone());
        self.send(method, path, Some(&who.token), act_as.as_deref(), body, &[])
            .await
    }

    async fn call_with(
        &self,
        who: &User,
        owner: Option<&User>,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let act_as = owner.map(|o| o.id.clone());
        self.send(
            method,
            path,
            Some(&who.token),
            act_as.as_deref(),
            body,
            headers,
        )
        .await
    }
}

struct User {
    token: String,
    id: String,
}

fn ok(reply: &Reply, what: &str) {
    assert!(
        reply.status.is_success(),
        "{what}: {} {:?}",
        reply.status,
        reply.body
    );
}

fn etag(reply: &Reply) -> String {
    reply.body["__etag"].as_str().unwrap().to_owned()
}

async fn until<F, Fut>(what: &str, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    assert!(
        postit_jobs::testkit::wait_until_async(Duration::from_secs(10), check).await,
        "timed out waiting for {what}"
    );
}

fn multipart(parts: &[(&str, Option<&str>, &str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, filename, content_type, bytes) in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        let disposition = match filename {
            Some(f) => format!("form-data; name=\"{name}\"; filename=\"{f}\""),
            None => format!("form-data; name=\"{name}\""),
        };
        out.extend_from_slice(format!("Content-Disposition: {disposition}\r\n").as_bytes());
        out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

fn png() -> Vec<u8> {
    let byte = |v: u32| u8::try_from(v * 4).unwrap_or(255);
    let img = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba([byte(x), byte(y), byte(x ^ y), 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

struct Flow {
    api: Api,
    owner: User,
    delegate: User,
    sink: Sink,
    running: RunningServer,
    oauth_provider: MockOAuthProvider,
    ai_server: MockServer,
    _dir: tempfile::TempDir,
}

async fn user(api: &Api, issuer: &TestIssuer, admin: Option<&User>, sub: &str) -> User {
    let email = format!("{sub}@postit.test");
    // The server accepts tokens of its configured web client only ().
    let claims = json!({
        "sub": sub,
        "iss": issuer.issuer_url().to_string().trim_end_matches('/'),
        "aud": "postit",
        "azp": CLIENT_ID_WEB,
        "exp": chrono::Utc::now().timestamp() + 3600,
        "email": email,
        "email_verified": true,
        "name": sub,
    });
    let token = issuer.mint(&claims, jsonwebtoken::Algorithm::RS256);
    let me = api
        .send(Method::GET, "/api/v1/me", Some(&token), None, None, &[])
        .await;
    ok(&me, "me");
    let id = me.body["id"].as_str().unwrap().to_owned();
    if let Some(admin) = admin {
        let approved = api
            .send(
                Method::PATCH,
                &format!("/api/v1/users/{id}"),
                Some(&admin.token),
                None,
                Some(json!({"status": "active"})),
                &[],
            )
            .await;
        ok(&approved, "approve");
    }
    User { token, id }
}

async fn start(pool: &PgPool) -> Flow {
    let issuer = TestIssuer::start().await;
    let media = tempfile::tempdir().unwrap();
    let dir = config_dir(&issuer, media.path());
    let settings = settings(&dir);
    let sink = Sink::start(Script::default()).await;
    let provider = MockOAuthProvider::start().await;
    provider.set_client(CLIENT_ID, CLIENT_SECRET);
    let oauth =
        MockAgent::oauth(&provider.base_url()).with_platform(PlatformId::new(OAUTH).unwrap());
    oauth.script().set_oauth_accounts(1);
    let mut registry = compose::agent_registry(&settings).unwrap();
    registry
        .register(std::sync::Arc::new(oauth) as std::sync::Arc<dyn Agent>)
        .unwrap();

    let api_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let worker_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let running = start_with_agents(
        StartOptions {
            env: Environment::Development,
            role: Role::All,
            settings,
            db: Some(postit_data::Db::from_pool(pool.clone())),
            api_listener: Some(api_listener),
            worker_listener: Some(worker_listener),
            internal_listener: None,
        },
        Some(registry),
    )
    .await
    .unwrap();
    let addr: SocketAddr = running.api_addr.unwrap();
    #[allow(clippy::disallowed_methods)] // a test client against a local listener
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let api = Api {
        http,
        base: format!("http://{addr}"),
    };
    until("the API to be ready", || async {
        api.send(Method::GET, "/ready", None, None, None, &[])
            .await
            .status
            == StatusCode::OK
    })
    .await;
    let admin = user(&api, &issuer, None, "admin").await;
    let owner = user(&api, &issuer, Some(&admin), "owner").await;
    let delegate = user(&api, &issuer, Some(&admin), "delegate").await;
    Flow {
        api,
        owner,
        delegate,
        sink,
        running,
        oauth_provider: provider,
        ai_server: MockServer::start().await,
        _dir: dir,
    }
}

impl Flow {
    /// Connects accounts for the owner through the check-then-confirm flow and returns the
    /// first connected account's ID.
    async fn confirm(&self, connect_id: &str) -> String {
        let confirmed = self
            .api
            .call(
                &self.owner,
                None,
                Method::POST,
                &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
                Some(json!({})),
            )
            .await;
        ok(&confirmed, "confirm");
        confirmed.body["connected"][0]["account"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn connect_credentials(&self, platform: &str, values: Value) -> String {
        let check = self
            .api
            .call(
                &self.owner,
                None,
                Method::POST,
                &format!("/api/v1/accounts/credentials/{platform}"),
                Some(values),
            )
            .await;
        ok(&check, "credentials check");
        self.confirm(check.body["connect_id"].as_str().unwrap())
            .await
    }

    async fn connect_oauth(&self) -> String {
        let started = self
            .api
            .call(
                &self.owner,
                None,
                Method::POST,
                &format!("/api/v1/accounts/connect/{OAUTH}"),
                Some(json!({"return_to": DONE})),
            )
            .await;
        ok(&started, "oauth start");
        let url = Url::parse(started.body["authorize_url"].as_str().unwrap()).unwrap();
        let code = self.oauth_provider.approve(&url);
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        let callback = self
            .api
            .send(
                Method::GET,
                &format!("/api/v1/platforms/{OAUTH}/oauth/callback?code={code}&state={state}"),
                None,
                None,
                None,
                &[],
            )
            .await;
        assert_eq!(
            callback.status,
            StatusCode::SEE_OTHER,
            "{:?}",
            callback.body
        );
        let location = Url::parse(callback.body["__location"].as_str().unwrap()).unwrap();
        let pending = location
            .query_pairs()
            .find(|(k, _)| k == "connect_id")
            .map_or_else(
                || unreachable!("no connect_id in {location}"),
                |(_, v)| v.into_owned(),
            );
        self.confirm(&pending).await
    }

    async fn job(&self, who: &User, owner: Option<&User>, id: &str) -> Reply {
        self.api
            .call(
                who,
                owner,
                Method::GET,
                &format!("/api/v1/publish-jobs/{id}"),
                None,
            )
            .await
    }

    /// Waits for every delivery of the job to reach `state`.
    async fn wait_deliveries(&self, who: &User, owner: Option<&User>, job: &str, state: &str) {
        until(&format!("job {job} to reach {state}"), || async {
            let reply = self.job(who, owner, job).await;
            reply.body["deliveries"]
                .as_array()
                .is_some_and(|d| !d.is_empty() && d.iter().all(|d| d["state"] == state))
        })
        .await;
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn owner_then_publish_delegate_run_the_whole_flow(pool: PgPool) {
    let f = start(&pool).await;
    let (owner, delegate, api) = (&f.owner, &f.delegate, &f.api);
    {
        let mut conn = pool.acquire().await.unwrap();
        postit_data::app_settings::AppSettingsRepo::seed_defaults(&mut conn, true)
            .await
            .unwrap();
    }

    // Connect a mock account by credentials, one through OAuth, and the email sending method.
    let mock_a = f
        .connect_credentials(
            "mock",
            json!({"account_name": "main", "token": "s3cret-token-value"}),
        )
        .await;
    let oauth_a = f.connect_oauth().await;
    let email_a = f
        .connect_credentials(
            "email",
            json!({
                "transport": "smtp",
                "from_name": "Corner Bakery",
                "from_address": FROM,
                "postal_address": "12 Main St, Springfield",
                "host": "127.0.0.1",
                "port": f.sink.port(),
                "tls": "none",
            }),
        )
        .await;
    let accounts = api
        .call(owner, None, Method::GET, "/api/v1/accounts", None)
        .await;
    assert_eq!(accounts.body["total"], 3, "{:?}", accounts.body);

    // Import consented contacts into a list and wait for the import.
    let list = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/contact-lists",
            Some(json!({"name": "Customers"})),
        )
        .await;
    ok(&list, "list");
    let list_id = list.body["id"].as_str().unwrap().to_owned();
    let options = json!({
        "attest_consent": true,
        "list_id": list_id,
        "columns": {"email": "email", "consent_source": "consent"},
    });
    let csv = b"email,consent\nana@reader.example,signup\nben@reader.example,signup\n";
    let body = multipart(&[
        (
            "options",
            None,
            "application/json",
            options.to_string().as_bytes(),
        ),
        ("file", Some("contacts.csv"), "text/csv", csv),
    ]);
    let import = api
        .http
        .post(format!("{}/api/v1/contacts/import", api.base))
        .bearer_auth(&owner.token)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert!(import.status().is_success(), "import: {}", import.status());
    let import: Value = import.json().await.unwrap_or(Value::Null);
    let import_id = import["id"].as_str().unwrap().to_owned();
    until("the contact import", || async {
        let reply = api
            .call(
                owner,
                None,
                Method::GET,
                &format!("/api/v1/contacts/imports/{import_id}"),
                None,
            )
            .await;
        reply.body["status"] == "completed"
    })
    .await;

    // Upload an image in a resumable session: start, two chunks, complete.
    let bytes = png();
    let total = bytes.len();
    let session = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/media/uploads",
            Some(json!({"filename": "bread.png", "size": total})),
        )
        .await;
    ok(&session, "upload start");
    let media_id = session.body["id"].as_str().unwrap().to_owned();
    let half = total / 2;
    for (from, to) in [(0, half), (half, total)] {
        let chunk = api
            .http
            .put(format!("{}/api/v1/media/uploads/{media_id}", api.base))
            .bearer_auth(&owner.token)
            .header("content-range", format!("bytes {from}-{}/{total}", to - 1))
            .body(bytes[from..to].to_vec())
            .send()
            .await
            .unwrap();
        assert!(chunk.status().is_success(), "chunk: {}", chunk.status());
    }
    let done = api
        .call(
            owner,
            None,
            Method::POST,
            &format!("/api/v1/media/uploads/{media_id}/complete"),
            None,
        )
        .await;
    ok(&done, "upload complete");

    // An AI provider (a wiremock chat endpoint) and a generation for the mock account.
    let completion = json!({
        "id": "c-1",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": json!({"variants": [
                {"target": mock_a, "text": "Fresh bread, out of the oven.", "tags": []}
            ]}).to_string()},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19}
    });
    Mock::given(method("POST"))
        .and(path_regex(".*/chat/completions$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion))
        .mount(&f.ai_server)
        .await;
    let provider = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/ai/providers",
            Some(json!({"opencode": {"provider": {"local": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "Local",
                "options": {"baseURL": format!("{}/v1", f.ai_server.uri()), "apiKey": "sk-test"},
                "models": {"m1": {"name": "Model one"}}
            }}}})),
        )
        .await;
    ok(&provider, "provider");
    let provider_id = provider.body["id"].as_str().unwrap().to_owned();
    let generated = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/ai/generate",
            Some(json!({
                "mode": "post",
                "prompt": "Announce fresh bread",
                "targets": [mock_a],
                "provider_id": provider_id,
                "model": "m1",
            })),
        )
        .await;
    ok(&generated, "generate");
    let generation_id = generated.body["generation_id"].as_str().unwrap().to_owned();
    let text = generated.body["draft"]["post"]["text"]
        .as_str()
        .unwrap()
        .to_owned();

    // Save the draft as a post with the generation, the image, and both broadcast accounts.
    let created = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/posts",
            Some(json!({
                "text": text,
                "disclose_ai_generated": true,
                "media": [{"media_id": media_id, "alt_text": "a loaf"}],
                "targets": [mock_a, oauth_a],
                "generation_id": generation_id,
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let post = created.body["id"].as_str().unwrap().to_owned();
    let tag = etag(&created);

    // Validate, then a dry run.
    let validated = api
        .call(
            owner,
            None,
            Method::POST,
            &format!("/api/v1/posts/{post}/validate"),
            Some(json!({})),
        )
        .await;
    ok(&validated, "validate");
    assert_eq!(validated.body["valid"], true, "{:?}", validated.body);
    let dry = api
        .call_with(
            owner,
            None,
            Method::POST,
            &format!("/api/v1/posts/{post}/publish"),
            Some(json!({"targets": [mock_a, oauth_a], "dry_run": true})),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(dry.status, StatusCode::CREATED, "{:?}", dry.body);
    f.wait_deliveries(owner, None, dry.body["id"].as_str().unwrap(), "simulated")
        .await;

    // Schedule the live publish two minutes ahead, find it in the calendar, then move it to
    // now and wait for the worker.
    let at = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
    let live = api
        .call_with(
            owner,
            None,
            Method::POST,
            &format!("/api/v1/posts/{post}/publish"),
            Some(json!({"targets": [mock_a, oauth_a], "confirm": true, "at": at})),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(live.status, StatusCode::CREATED, "{:?}", live.body);
    let job = live.body["id"].as_str().unwrap().to_owned();
    let from = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let to = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let calendar = api
        .call(
            owner,
            None,
            Method::GET,
            &format!(
                "/api/v1/calendar?from={}&to={}",
                from.replace('+', "%2B"),
                to.replace('+', "%2B")
            ),
            None,
        )
        .await;
    ok(&calendar, "calendar");
    assert!(
        calendar.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["job_id"] == job),
        "{:?}",
        calendar.body
    );
    let now = chrono::Utc::now().to_rfc3339();
    let moved = api
        .call_with(
            owner,
            None,
            Method::PATCH,
            &format!("/api/v1/publish-jobs/{job}"),
            Some(json!({"at": now})),
            &[("if-match", &etag(&live))],
        )
        .await;
    ok(&moved, "reschedule");
    f.wait_deliveries(owner, None, &job, "published").await;
    let notices = api
        .call(owner, None, Method::GET, "/api/v1/notifications", None)
        .await;
    ok(&notices, "notifications");
    assert!(
        notices.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "delivery_published"),
        "{:?}",
        notices.body
    );

    // An email campaign: an override with the email document and the list, published live.
    let mail_post = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/posts",
            Some(json!({"text": "Fresh bread", "targets": [email_a]})),
        )
        .await;
    assert_eq!(
        mail_post.status,
        StatusCode::CREATED,
        "{:?}",
        mail_post.body
    );
    let mail_id = mail_post.body["id"].as_str().unwrap().to_owned();
    let with_override = api
        .call_with(
            owner,
            None,
            Method::PUT,
            &format!("/api/v1/posts/{mail_id}/overrides/{email_a}"),
            Some(json!({
                "patch": {"email": {
                    "subject": "Fresh bread, {{first_name|friend}}",
                    "language": "en-US",
                    "blocks": [
                        {"type": "text", "text": "Hello {{first_name|there}}, come early."},
                        {"type": "button", "label": "Order", "url": "https://bakery.example/order"}
                    ],
                }},
                "options": {"lists": [list_id]},
            })),
            &[("if-match", &etag(&mail_post))],
        )
        .await;
    ok(&with_override, "override");
    let campaign = api
        .call_with(
            owner,
            None,
            Method::POST,
            &format!("/api/v1/posts/{mail_id}/publish"),
            Some(json!({"targets": [email_a], "confirm": true})),
            &[("if-match", &etag(&with_override))],
        )
        .await;
    assert_eq!(campaign.status, StatusCode::CREATED, "{:?}", campaign.body);
    f.wait_deliveries(
        owner,
        None,
        campaign.body["id"].as_str().unwrap(),
        "published",
    )
    .await;
    let mut recipients: Vec<String> = f
        .sink
        .received()
        .iter()
        .flat_map(|mail| mail.to.clone())
        .collect();
    recipients.sort();
    assert_eq!(recipients, ["ana@reader.example", "ben@reader.example"]);

    // History: jobs, calendar, notifications.
    let jobs = api
        .call(owner, None, Method::GET, "/api/v1/publish-jobs", None)
        .await;
    ok(&jobs, "jobs");
    assert!(jobs.body["total"].as_i64().unwrap() >= 3, "{:?}", jobs.body);

    // The same flow as a `publish` delegate acting in the owner's workspace.
    let grant = api
        .call(
            owner,
            None,
            Method::POST,
            "/api/v1/delegations",
            Some(json!({"delegate_id": delegate.id, "level": "publish"})),
        )
        .await;
    assert_eq!(grant.status, StatusCode::CREATED, "{:?}", grant.body);
    let campaign = api
        .call(
            delegate,
            Some(owner),
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "Spring"})),
        )
        .await;
    assert_eq!(campaign.status, StatusCode::CREATED, "{:?}", campaign.body);
    let delegated = api
        .call(
            delegate,
            Some(owner),
            Method::POST,
            "/api/v1/posts",
            Some(json!({
                "text": "Spring loaves",
                "campaign_id": campaign.body["id"],
                "targets": [mock_a],
            })),
        )
        .await;
    assert_eq!(
        delegated.status,
        StatusCode::CREATED,
        "{:?}",
        delegated.body
    );
    let delegated_id = delegated.body["id"].as_str().unwrap().to_owned();
    let valid = api
        .call(
            delegate,
            Some(owner),
            Method::POST,
            &format!("/api/v1/posts/{delegated_id}/validate"),
            Some(json!({})),
        )
        .await;
    assert_eq!(valid.body["valid"], true, "{:?}", valid.body);

    // The delegate schedules the live publish two minutes ahead, finds it in the owner's
    // calendar, then moves it to now and waits for the worker.
    let at = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
    let published = api
        .call_with(
            delegate,
            Some(owner),
            Method::POST,
            &format!("/api/v1/posts/{delegated_id}/publish"),
            Some(json!({"targets": [mock_a], "confirm": true, "at": at})),
            &[("if-match", &etag(&delegated))],
        )
        .await;
    assert_eq!(
        published.status,
        StatusCode::CREATED,
        "{:?}",
        published.body
    );
    assert_eq!(published.body["created_by"], delegate.id);
    assert_eq!(
        published.body["status"], "scheduled",
        "{:?}",
        published.body
    );
    let delegated_job = published.body["id"].as_str().unwrap().to_owned();
    let cal = api
        .call(
            delegate,
            Some(owner),
            Method::GET,
            &format!(
                "/api/v1/calendar?from={}&to={}",
                from.replace('+', "%2B"),
                to.replace('+', "%2B")
            ),
            None,
        )
        .await;
    ok(&cal, "delegate calendar");
    assert!(
        cal.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["job_id"] == delegated_job),
        "{:?}",
        cal.body
    );
    let now = chrono::Utc::now().to_rfc3339();
    let moved = api
        .call_with(
            delegate,
            Some(owner),
            Method::PATCH,
            &format!("/api/v1/publish-jobs/{delegated_job}"),
            Some(json!({"at": now})),
            &[("if-match", &etag(&published))],
        )
        .await;
    ok(&moved, "delegate reschedule");
    f.wait_deliveries(delegate, Some(owner), &delegated_job, "published")
        .await;

    // The delegate sends an email campaign to the owner's list.
    let mail_post = api
        .call(
            delegate,
            Some(owner),
            Method::POST,
            "/api/v1/posts",
            Some(json!({
                "text": "Spring loaves",
                "campaign_id": campaign.body["id"],
                "targets": [email_a],
            })),
        )
        .await;
    assert_eq!(
        mail_post.status,
        StatusCode::CREATED,
        "{:?}",
        mail_post.body
    );
    let mail_id = mail_post.body["id"].as_str().unwrap().to_owned();
    let with_override = api
        .call_with(
            delegate,
            Some(owner),
            Method::PUT,
            &format!("/api/v1/posts/{mail_id}/overrides/{email_a}"),
            Some(json!({
                "patch": {"email": {
                    "subject": "Spring loaves, {{first_name|friend}}",
                    "language": "en-US",
                    "blocks": [{"type": "text", "text": "Spring loaves are in."}],
                }},
                "options": {"lists": [list_id]},
            })),
            &[("if-match", &etag(&mail_post))],
        )
        .await;
    ok(&with_override, "delegate override");
    let sent = api
        .call_with(
            delegate,
            Some(owner),
            Method::POST,
            &format!("/api/v1/posts/{mail_id}/publish"),
            Some(json!({"targets": [email_a], "confirm": true})),
            &[("if-match", &etag(&with_override))],
        )
        .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.body);
    assert_eq!(sent.body["created_by"], delegate.id);
    f.wait_deliveries(
        delegate,
        Some(owner),
        sent.body["id"].as_str().unwrap(),
        "published",
    )
    .await;
    let spring = f
        .sink
        .received()
        .iter()
        .filter(|mail| {
            mail.header("subject")
                .is_some_and(|subject| subject.starts_with("Spring loaves"))
        })
        .count();
    assert_eq!(spring, 2, "one campaign mail per subscribed contact");
    f.running.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(40), f.running.handle).await;
    assert!(
        matches!(finished, Ok(Ok(Ok(())))),
        "clean shutdown: {finished:?}"
    );
}
