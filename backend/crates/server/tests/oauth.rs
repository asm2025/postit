//! The worker role's OAuth wiring: the recurring jobs, the purge's revokes, and the OAuth step
//! of user deletion, against the mock OAuth provider.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::{MockAgent, MockScript};
use postit_config::{Environment, Settings};
use postit_core::{Agent, AgentRegistry, PlatformId};
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::testkit::seed_user;
use postit_identity::admin::UserAdminService;
use postit_jobs::{JobConsole, JobQueue};
use postit_mail::MailOutbox;
use postit_oauth::{AccountService, CallbackParams};
use postit_server::{Role, RunningServer, StartOptions, compose, start_with_agents};
use secrecy::SecretString;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";
const CLIENT_ID: &str = "postit-app";
const CLIENT_SECRET: &str = "postit-app-secret-value";
const DONE: &str = "http://127.0.0.1:44315/oauth-done";

fn config_dir(media: &std::path::Path) -> tempfile::TempDir {
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
issuer = "http://127.0.0.1:1/"
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
signing_key = "oauth-media-signing-key-0123456789"
[platforms.{OAUTH}]
client_id = "{CLIENT_ID}"
client_secret = "{CLIENT_SECRET}"
"#,
        media = media.display()
    );
    std::fs::write(dir.path().join("development.toml"), dev).unwrap();
    dir
}

fn settings(dir: &tempfile::TempDir) -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "oauth-key"),
    ];
    postit_config::load_with(
        Environment::Development,
        dir.path(),
        vars.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .unwrap()
}

fn registry(agent: &Arc<dyn Agent>) -> AgentRegistry {
    let mut registry = AgentRegistry::new();
    registry.register(Arc::clone(agent)).unwrap();
    registry
}

struct Fixture {
    pool: PgPool,
    settings: Settings,
    provider: MockOAuthProvider,
    script: MockScript,
    accounts: AccountService,
    running: RunningServer,
    _dir: tempfile::TempDir,
    _media: tempfile::TempDir,
}

/// A worker over `mock_oauth` pointed at a mock provider, and an account service over the
/// same database, vault, and agent to seed grants and results with.
async fn fixture(pool: &PgPool) -> Fixture {
    let media = tempfile::tempdir().unwrap();
    let dir = config_dir(media.path());
    let settings = settings(&dir);
    let provider = MockOAuthProvider::start().await;
    provider.set_client(CLIENT_ID, CLIENT_SECRET);
    let inner =
        MockAgent::oauth(&provider.base_url()).with_platform(PlatformId::new(OAUTH).unwrap());
    let script = inner.script();
    script.set_oauth_accounts(1);
    let agent: Arc<dyn Agent> = Arc::new(inner);
    postit_data::platforms::PlatformRepo::upsert_registered(
        pool,
        &[(PlatformId::new(OAUTH).unwrap(), OAUTH.to_owned())],
    )
    .await
    .unwrap();

    let agents = registry(&agent);
    let configs = postit_server::platforms::resolve_platform_configs(&settings, &agents).unwrap();
    let ids = compose::system_ids();
    let clock = compose::system_clock();
    let media_service = compose::media_service(&settings, pool, &ids, &clock).unwrap();
    let http = postit_http::build_client(&settings.http).unwrap();
    let vault = postit_vault::Vault::from_settings(&settings.vault).unwrap();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
    let outbox = MailOutbox::new(jobs.clone());
    let notifier = compose::notifier(&settings, &ids, &clock, &jobs, &outbox);
    let accounts = compose::account_service(
        &settings,
        Environment::Development,
        pool,
        &http,
        &vault,
        Arc::new(postit_oauth::PlatformCatalog::new(agents, configs)),
        (
            Arc::clone(&media_service.store) as Arc<dyn postit_core::MediaStore>,
            Arc::clone(&media_service.host) as Arc<dyn postit_core::MediaHost>,
        ),
        &notifier,
        &ids,
        &clock,
    )
    .unwrap();

    let running = start_with_agents(
        StartOptions {
            env: Environment::Development,
            role: Role::Worker,
            settings: settings.clone(),
            db: Some(postit_data::Db::from_pool(pool.clone())),
            api_listener: None,
            worker_listener: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
            internal_listener: None,
        },
        Some(registry(&agent)),
    )
    .await
    .unwrap();
    Fixture {
        pool: pool.clone(),
        settings,
        provider,
        script,
        accounts,
        running,
        _dir: dir,
        _media: media,
    }
}

impl Fixture {
    fn console(&self) -> JobConsole {
        let ids = compose::system_ids();
        let clock = compose::system_clock();
        JobConsole::new(
            self.pool.clone(),
            JobQueue::new(self.pool.clone(), Arc::clone(&ids), Arc::clone(&clock)),
            compose::console_catalog(&self.settings),
            clock,
        )
    }

    /// `scope` starts a connection, the provider approves it, and the callback runs; returns
    /// the `connect_id` of the pending result.
    async fn connected_by(&self, scope: &OwnerScope) -> String {
        let platform = PlatformId::new(OAUTH).unwrap();
        let start = self
            .accounts
            .start_oauth(scope, &platform, DONE)
            .await
            .unwrap();
        let code = self.provider.approve(&start.authorize_url);
        let state = param(&start.authorize_url, "state").unwrap();
        let target = self
            .accounts
            .oauth_callback(
                OAUTH,
                CallbackParams {
                    code: Some(SecretString::from(code)),
                    state: Some(state),
                    error: None,
                },
            )
            .await;
        param(&target, "connect_id").unwrap_or_else(|| panic!("callback failed: {target}"))
    }

    /// Waits until the job `id` has succeeded; panics when it ends any other way or runs too
    /// long.
    async fn wait_for_job(&self, id: Uuid) {
        for _ in 0..300 {
            let state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .unwrap();
            match state.as_str() {
                "succeeded" => return,
                "queued" | "running" | "failed" => {}
                other => panic!("job {id} ended as {other}"),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("job {id} did not succeed in time");
    }

    async fn stop(self) {
        self.running.shutdown.cancel();
        let finished = tokio::time::timeout(Duration::from_secs(40), self.running.handle).await;
        assert!(matches!(finished, Ok(Ok(Ok(())))), "{finished:?}");
    }
}

fn param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn worker_registers_oauth_recurring_jobs(pool: PgPool) {
    let f = fixture(&pool).await;
    let console = f.console();
    let names: Vec<String> = console
        .recurring()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert!(
        names.contains(&"refresh_credentials".to_owned()),
        "{names:?}"
    );
    assert!(names.contains(&"oauth_state_purge".to_owned()), "{names:?}");

    // The worker has handlers registered for both: a manual run of each succeeds.
    for name in ["refresh_credentials", "oauth_state_purge"] {
        let mut conn = pool.acquire().await.unwrap();
        let job = console.trigger(&mut conn, name).await.unwrap();
        drop(conn);
        f.wait_for_job(job).await;
    }
    f.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn worker_purge_revokes_a_discarded_result(pool: PgPool) {
    let f = fixture(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let connect_id = f.connected_by(&scope).await;
    // Confirm nothing: the result is discarded, its tokens left for the purge to revoke.
    let confirmed = f
        .accounts
        .confirm(&scope, &connect_id, Some(vec![]), AuditContext::default())
        .await
        .unwrap();
    assert!(confirmed.connected.is_empty());
    assert!(f.provider.revoked().is_empty());

    let console = f.console();
    let mut conn = pool.acquire().await.unwrap();
    let job = console
        .trigger(&mut conn, "oauth_state_purge")
        .await
        .unwrap();
    drop(conn);
    f.wait_for_job(job).await;

    assert_eq!(f.provider.revoked().len(), 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM oauth_states").await, 0);
    f.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_user_revokes_oauth_grants(pool: PgPool) {
    let f = fixture(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let actor = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);

    // A connected grant for one remote user.
    let connect_id = f.connected_by(&scope).await;
    let confirmed = f
        .accounts
        .confirm(&scope, &connect_id, None, AuditContext::default())
        .await
        .unwrap();
    assert_eq!(confirmed.connected.len(), 1);
    // And a pending result for another.
    f.script.set_exchange(postit_agent_mock::ExchangeScript {
        remote_user_id: Some("other-user".to_owned()),
        ..postit_agent_mock::ExchangeScript::default()
    });
    f.connected_by(&scope).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM oauth_states").await, 1);
    assert!(f.provider.revoked().is_empty());

    let ids = compose::system_ids();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), compose::system_clock());
    UserAdminService::new(pool.clone(), ids, jobs.clone(), MailOutbox::new(jobs))
        .delete_user(actor, owner, AuditContext::default())
        .await
        .unwrap();
    let job: Uuid = sqlx::query_scalar("SELECT id FROM jobs WHERE job_type = 'delete_user'")
        .fetch_one(&pool)
        .await
        .unwrap();
    f.wait_for_job(job).await;

    assert_eq!(f.provider.revoked().len(), 2, "{:?}", f.provider.revoked());
    for table in [
        "users WHERE id = $1",
        "account_grants WHERE owner_id = $1",
        "oauth_states WHERE user_id = $1",
    ] {
        let rows: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .bind(owner.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows, 0, "{table}");
    }
    f.stop().await;
}

/// Serving agents other than the compiled-in ones is a test seam: production refuses it
/// before anything starts.
#[tokio::test]
async fn an_injected_registry_is_refused_in_production() {
    let media = tempfile::tempdir().unwrap();
    let dir = config_dir(media.path());
    let started = start_with_agents(
        StartOptions {
            env: Environment::Production,
            role: Role::Worker,
            settings: settings(&dir),
            db: None,
            api_listener: None,
            worker_listener: None,
            internal_listener: None,
        },
        Some(AgentRegistry::new()),
    )
    .await;
    let Err(error) = started else {
        panic!("production started with an injected registry");
    };
    assert!(error.to_string().contains("injected"), "{error}");
}
