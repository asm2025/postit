use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use postit_config::{Environment, Settings};
use postit_identity::testkit::TestIssuer;
use postit_server::{Role, StartOptions, start};
use sqlx::PgPool;

fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| unreachable!("connect: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap_or_default();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap_or_default();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, response)
}

/// `/ready` polling must not exhaust the 127.0.0.1 bucket before the assertions.
const QUIET_LIMITS: &str = "[rate_limit.unauthenticated]
rate_per_minute = 100000
burst = 100000";

fn config_dir(issuer: &TestIssuer, extra: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let base = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/default.toml"
    ))
    .unwrap_or_else(|e| unreachable!("default.toml: {e}"));
    std::fs::write(dir.path().join("default.toml"), base).unwrap_or_default();
    let issuer_url = issuer.issuer_url().to_string();
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
{extra}
"#,
        issuer = issuer_url.trim_end_matches('/')
    );
    std::fs::write(dir.path().join("development.toml"), dev).unwrap_or_default();
    dir
}

fn settings(dir: &tempfile::TempDir) -> Settings {
    postit_config::load_with(
        Environment::Development,
        dir.path(),
        [
            (
                "POSTIT__DATABASE__USERNAME".to_string(),
                "unused".to_string(),
            ),
            (
                "POSTIT__DATABASE__PASSWORD".to_string(),
                "unused".to_string(),
            ),
            (
                "POSTIT__AUDIT__PSEUDONYM_KEY".to_string(),
                "smoke-key".to_string(),
            ),
        ],
    )
    .unwrap_or_else(|e| unreachable!("config: {e}"))
}

fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("bind: {e}"))
}

async fn wait_ready(addr: SocketAddr) -> bool {
    tokio::task::spawn_blocking(move || {
        for _ in 0..100 {
            if get(addr, "/ready").0 == 200 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    })
    .await
    .unwrap_or(false)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn role_all_serves_health_and_ready_on_both_ports_and_shuts_down(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::All,
        settings: settings(&dir),
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: Some(listener()),
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));

    let (Some(api_addr), Some(worker_addr)) = (running.api_addr, running.worker_addr) else {
        unreachable!("role all binds both ports");
    };
    assert!(wait_ready(api_addr).await, "api port must become ready");
    assert!(
        wait_ready(worker_addr).await,
        "worker port must become ready"
    );
    let health = tokio::task::spawn_blocking(move || {
        (get(api_addr, "/health").0, get(worker_addr, "/health").0)
    })
    .await
    .unwrap_or_default();
    assert_eq!(health, (200, 200));

    // A real API request through the real composition root: the client-IP layer needs the
    // peer address from `into_make_service_with_connect_info`, else this is a 500.
    let (status, body) = tokio::task::spawn_blocking(move || get(api_addr, "/api/v1/auth/config"))
        .await
        .unwrap_or_default();
    assert_eq!(
        status, 200,
        "API request must not 500 for missing ConnectInfo: {body}"
    );
    assert!(body.contains("client_id"), "auth config body: {body}");

    let probe = tokio::task::spawn_blocking(move || postit_server::healthcheck::probe(api_addr))
        .await
        .unwrap_or_else(|e| unreachable!("join: {e}"));
    assert!(probe.is_ok(), "healthcheck against the API port: {probe:?}");

    running.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(40), running.handle).await;
    assert!(
        matches!(finished, Ok(Ok(Ok(())))),
        "clean shutdown, got {finished:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn role_worker_serves_only_the_worker_port(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::Worker,
        settings: settings(&dir),
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: None,
        worker_listener: Some(listener()),
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));
    assert!(running.api_addr.is_none());
    let Some(worker_addr) = running.worker_addr else {
        unreachable!("worker role binds the worker port");
    };
    assert!(
        wait_ready(worker_addr).await,
        "worker readiness comes from WorkerHealth"
    );
    running.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(40), running.handle).await;
    assert!(
        matches!(finished, Ok(Ok(Ok(())))),
        "clean shutdown, got {finished:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn wildcard_cors_origin_is_a_startup_error(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, "[cors]\nallowed_origins = [\"*\"]");
    let result = start(StartOptions {
        env: Environment::Development,
        role: Role::Api,
        settings: settings(&dir),
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: None,
    })
    .await;
    let message = result.err().map(|e| format!("{e:#}")).unwrap_or_default();
    assert!(message.contains("cors.allowed_origins"), "got: {message}");
}

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    std::fs::write(dir.path().join("index.html"), "<html>app</html>").unwrap_or_default();
    dir
}

async fn shuts_down_cleanly(running: postit_server::RunningServer) {
    running.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(40), running.handle).await;
    assert!(
        matches!(finished, Ok(Ok(Ok(())))),
        "clean shutdown, got {finished:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn web_root_without_index_html_fails_startup(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let empty = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let mut s = settings(&dir);
    s.server.web.enabled = true;
    s.server.web.root = Some(empty.path().to_path_buf());
    let result = start(StartOptions {
        env: Environment::Development,
        role: Role::Api,
        settings: s,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: None,
    })
    .await;
    let err = result.err().map(|e| format!("{e:#}")).unwrap_or_default();
    assert!(err.contains("index.html"), "got {err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn web_bind_serves_the_app_on_its_own_port(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let site = site();
    let mut s = settings(&dir);
    s.server.web.enabled = true;
    s.server.web.root = Some(site.path().to_path_buf());
    s.server.web.bind = Some("127.0.0.1:0".to_string());
    s.server.web.api_base_url = "http://127.0.0.1:1".parse().ok();
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::All,
        settings: s,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: Some(listener()),
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));
    let Some(web_addr) = running.web_addr else {
        unreachable!("server.web.bind gives the app its own listener");
    };
    let (status, body) = tokio::task::spawn_blocking(move || get(web_addr, "/users/1"))
        .await
        .unwrap_or_default();
    assert_eq!(status, 200);
    assert!(body.contains("<html>app</html>"), "{body}");
    let (_, config) = tokio::task::spawn_blocking(move || get(web_addr, "/config.json"))
        .await
        .unwrap_or_default();
    assert!(config.contains(r#""ENV":"development""#), "{config}");
    assert!(config.contains("http://127.0.0.1:1"), "{config}");
    let (status, body) = tokio::task::spawn_blocking(move || get(web_addr, "/api/v1/me"))
        .await
        .unwrap_or_default();
    assert_eq!(status, 404);
    assert!(!body.contains("<html>"), "{body}");
    shuts_down_cleanly(running).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn web_on_the_api_port_keeps_api_404s_and_skips_rate_limits(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    // A tiny bucket, so the rate limiter would trip well inside the loop below.
    let dir = config_dir(
        &issuer,
        "[rate_limit.unauthenticated]
rate_per_minute = 1
burst = 3",
    );
    let site = site();
    let mut s = settings(&dir);
    s.server.web.enabled = true;
    s.server.web.root = Some(site.path().to_path_buf());
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::Api,
        settings: s,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: None,
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));
    let Some(api_addr) = running.api_addr else {
        unreachable!("api role binds the api port");
    };
    let (spa, nope, health, statuses) = tokio::task::spawn_blocking(move || {
        let spa = get(api_addr, "/users/1");
        let nope = get(api_addr, "/api/v1/nope");
        let health = get(api_addr, "/health");
        let statuses: Vec<u16> = (0..10).map(|_| get(api_addr, "/").0).collect();
        (spa, nope, health, statuses)
    })
    .await
    .unwrap_or_default();
    assert_eq!(spa.0, 200);
    assert!(spa.1.contains("<html>app</html>"));
    assert_eq!(nope.0, 404);
    assert!(
        nope.1.to_lowercase().contains("application/problem+json"),
        "{}",
        nope.1
    );
    assert_eq!(health.0, 200);
    assert_eq!(statuses, vec![200; 10]);
    shuts_down_cleanly(running).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn worker_role_ignores_server_web(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let empty = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let mut s = settings(&dir);
    s.server.web.enabled = true;
    s.server.web.root = Some(empty.path().to_path_buf());
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::Worker,
        settings: s,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: None,
        worker_listener: Some(listener()),
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));
    let Some(worker_addr) = running.worker_addr else {
        unreachable!("worker role binds the worker port");
    };
    assert!(wait_ready(worker_addr).await);
    let (status, _) = tokio::task::spawn_blocking(move || get(worker_addr, "/"))
        .await
        .unwrap_or_default();
    assert_eq!(status, 404);
    shuts_down_cleanly(running).await;
}

#[test]
fn role_defaults_to_all_in_development_only() {
    assert_eq!(
        Role::resolve(None, Environment::Development).ok(),
        Some(Role::All)
    );
    assert!(Role::resolve(None, Environment::Production).is_err());
    assert_eq!(
        Role::resolve(Some("worker"), Environment::Qa).ok(),
        Some(Role::Worker)
    );
    assert!(Role::resolve(Some("both"), Environment::Development).is_err());
}
