//! The composition root (plan 02, `postit-server`). `main.rs` calls [`run_from_env`];
//! tests call [`start`] with their own pool and ephemeral listeners.
#![warn(missing_docs)]

/// Wiring of settings into services, per role.
pub mod compose;
/// The `healthcheck` subcommand used by container health checks.
pub mod healthcheck;
/// Platform configuration, availability, and the vault startup step.
pub mod platforms;
/// Process roles.
pub mod role;
/// TLS configuration and the shared serve loop.
pub mod serve;
/// Tracing setup.
pub mod telemetry;
pub mod unavailable;
/// The `vault_rotate` job.
pub mod vault_rotate;
pub mod web;

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use postit_api::{ApiSettings, AppState, Limits, api_router, probe_router};
use postit_config::{Environment, Settings};
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use postit_jobs::{JobQueue, Worker};
use postit_mail::MailOutbox;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub use role::Role;

/// Everything `start` needs.
pub struct StartOptions {
    /// Deployment environment.
    pub env: Environment,
    /// Which parts of the process to run.
    pub role: Role,
    /// The validated configuration.
    pub settings: Settings,
    /// Use this pool instead of connecting from `settings.database` (tests).
    pub db: Option<postit_data::Db>,
    /// Pre-bound API listener; binds from settings when unset.
    pub api_listener: Option<TcpListener>,
    /// Pre-bound worker listener; binds from settings when unset.
    pub worker_listener: Option<TcpListener>,
    /// Pre-bound identity-mail listener; binds `identity_mail.bind` when unset and enabled.
    pub internal_listener: Option<TcpListener>,
}

/// A started server and the means to stop it.
pub struct RunningServer {
    /// Address the API listens on, when the role runs it.
    pub api_addr: Option<SocketAddr>,
    /// Address the worker's health port listens on, when the role runs it.
    pub worker_addr: Option<SocketAddr>,
    /// Set only when `server.web.bind` gives the web app its own listener.
    pub web_addr: Option<SocketAddr>,
    /// Set only when `identity_mail.enabled` gives the Zitadel webhook its own listener.
    pub internal_addr: Option<SocketAddr>,
    /// Cancel to begin a graceful shutdown.
    pub shutdown: CancellationToken,
    /// Resolves when every task has stopped.
    pub handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

fn bind(host: &str, port: u16) -> anyhow::Result<TcpListener> {
    TcpListener::bind((host, port)).with_context(|| format!("binding {host}:{port}"))
}

/// Connects (unless a pool was injected), migrates both schemas, runs the bootstrap check, then
/// checks the vault and, in API roles, records the platforms and seeds the app settings.
async fn open_database(
    env: Environment,
    role: Role,
    settings: &Settings,
    db: Option<postit_data::Db>,
    setup: &PlatformSetup,
) -> anyhow::Result<sqlx::PgPool> {
    let db = match db {
        Some(db) => db,
        None => postit_data::Db::connect(&settings.database)
            .await
            .context("connecting to the database")?,
    };
    db.run_migrations()
        .await
        .context("running postit migrations")?;
    postit_jobs::migrate(db.pool())
        .await
        .context("running job storage migrations")?;
    let pool = db.pool().clone();
    postit_identity::bootstrap::check_startup(&pool, &settings.auth.bootstrap, env).await?;
    platforms::check_vault_and_platforms(
        &pool,
        &platforms::PlatformStartup {
            vault: &setup.vault,
            role,
            registry: &setup.agents,
            configs: &setup.configs,
            public_url: &settings.server.public_url,
        },
    )
    .await?;
    if role.runs_api() {
        let mut conn = pool.acquire().await.context("acquiring a connection")?;
        postit_data::app_settings::AppSettingsRepo::seed_defaults(
            &mut conn,
            settings.publishing.live,
        )
        .await
        .context("seeding the app settings")?;
    }
    Ok(pool)
}

/// Everything that can be rejected before the database is touched or any task starts.
struct Prepared {
    api_settings: Option<ApiSettings>,
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
    api_listener: Option<TcpListener>,
    worker_listener: Option<TcpListener>,
    web_router: Option<axum::Router>,
    web_listener: Option<TcpListener>,
    internal_listener: Option<TcpListener>,
    setup: PlatformSetup,
}

/// The vault and the compiled-in agents with their resolved `[platforms]` tables.
struct PlatformSetup {
    vault: postit_vault::Vault,
    agents: postit_core::AgentRegistry,
    configs: platforms::PlatformConfigs,
}

/// Validates the settings the roles need (CORS origins, trusted proxies, TLS files, vault keys,
/// platform tables) and binds the ports, so a bad config or a taken port fails startup with
/// nothing running. Installs the outbound-URL redactor first, before any request is sent.
async fn prepare(
    env: Environment,
    role: Role,
    settings: &Settings,
    api_listener: Option<TcpListener>,
    worker_listener: Option<TcpListener>,
    internal_listener: Option<TcpListener>,
) -> anyhow::Result<Prepared> {
    install_http_redactor(settings);
    compose::refuse_mock_in_production(env)?;
    let agents = compose::agent_registry()?;
    let mut ids: Vec<&str> = agents
        .iter()
        .map(|a| a.descriptor().platform.as_str())
        .collect();
    ids.sort_unstable();
    tracing::info!(platforms = ?ids, "registered agents");
    let setup = PlatformSetup {
        vault: postit_vault::Vault::from_settings(&settings.vault).context("building the vault")?,
        configs: platforms::resolve_platform_configs(settings, &agents)?,
        agents,
    };
    let api_settings = if role.runs_api() {
        Some(ApiSettings::from_settings(env, settings).map_err(anyhow::Error::msg)?)
    } else {
        None
    };
    let tls = if settings.server.tls.enabled {
        let (Some(cert), Some(key)) = (
            &settings.server.tls.cert_path,
            &settings.server.tls.key_path,
        ) else {
            anyhow::bail!(
                "server.tls.enabled requires server.tls.cert_path and server.tls.key_path"
            );
        };
        Some(serve::tls_config(cert, key).await?)
    } else {
        None
    };
    let api_listener = match (role.runs_api(), api_listener) {
        (true, Some(l)) => Some(l),
        (true, None) => Some(bind(&settings.server.host, settings.server.api_port)?),
        (false, _) => None,
    };
    let worker_listener = match (role.runs_worker(), worker_listener) {
        (true, Some(l)) => Some(l),
        (true, None) => Some(bind(&settings.server.host, settings.server.worker_port)?),
        (false, _) => None,
    };
    let web = if role.runs_api() {
        web::from_settings(env, settings)?
    } else {
        None
    };
    let (web_router, web_listener) = match web {
        Some(web::WebHosting {
            router,
            bind: Some(addr),
        }) => {
            let listener = TcpListener::bind(addr)
                .with_context(|| format!("binding server.web.bind {addr}"))?;
            (Some(router), Some(listener))
        }
        Some(web::WebHosting { router, bind: None }) => (Some(router), None),
        None => (None, None),
    };
    let internal_listener = match (
        role.runs_api() && settings.identity_mail.enabled,
        internal_listener,
        settings.identity_mail.bind_addr(),
    ) {
        (true, Some(l), _) => Some(l),
        (true, None, Some(addr)) => Some(
            TcpListener::bind(addr)
                .with_context(|| format!("binding identity_mail.bind {addr}"))?,
        ),
        _ => None,
    };
    Ok(Prepared {
        api_settings,
        tls,
        api_listener,
        worker_listener,
        web_router,
        web_listener,
        internal_listener,
        setup,
    })
}

/// Installs the deployment's process-wide outbound-URL redactor (see [`compose::redactor`]).
/// A second `start` in one process (tests) keeps the first deployment's patterns.
fn install_http_redactor(settings: &Settings) {
    if !postit_http::install_redactor(compose::redactor(settings)) {
        tracing::debug!("http redactor already installed");
    }
}

/// Logs the effective configuration with secrets redacted.
pub fn log_effective_config(settings: &Settings) {
    if let Ok(dump) = settings.redacted_dump() {
        tracing::info!(config = %dump, "effective configuration");
    }
}

/// Startup per plan 02: validate, migrate, identity, bootstrap check, services, job registry, serve.
///
/// # Errors
///
/// Any startup failure; nothing is left running when this returns `Err`.
#[allow(clippy::too_many_lines)] // one linear startup sequence; splitting it hides the order
pub async fn start(opts: StartOptions) -> anyhow::Result<RunningServer> {
    let StartOptions {
        env,
        role,
        settings,
        db,
        api_listener,
        worker_listener,
        internal_listener,
    } = opts;
    tracing::info!(role = ?role, "starting postit");
    log_effective_config(&settings);
    let Prepared {
        api_settings,
        tls,
        api_listener,
        worker_listener,
        web_router,
        web_listener,
        internal_listener,
        setup,
    } = prepare(
        env,
        role,
        &settings,
        api_listener,
        worker_listener,
        internal_listener,
    )
    .await?;

    let pool = open_database(env, role, &settings, db, &setup).await?;
    let PlatformSetup {
        vault,
        agents,
        configs,
    } = setup;

    let http = postit_http::build_client(&settings.http).context("building the HTTP client")?;
    let ids = compose::system_ids();
    let clock = compose::system_clock();
    let accounts = compose::account_service(
        &settings,
        &pool,
        &http,
        &vault,
        Arc::new(postit_oauth::PlatformCatalog::new(agents, configs)),
        &ids,
        &clock,
    )?;
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
    let outbox = MailOutbox::new(jobs.clone());
    let mailer = if role.runs_worker() || settings.identity_mail.enabled {
        Some(compose::mailer(&settings)?)
    } else {
        None
    };
    let shutdown = CancellationToken::new();
    let grace = settings.server.shutdown_timeout;

    let mut tasks: JoinSet<anyhow::Result<()>> = JoinSet::new();
    let mut background: JoinSet<()> = JoinSet::new();
    let services = Services {
        settings: &settings,
        pool: &pool,
        http: &http,
        ids: &ids,
        clock: &clock,
        jobs: &jobs,
        outbox: &outbox,
        mailer: mailer.as_ref(),
        vault: &vault,
        accounts: &accounts,
    };

    let auth = if role.runs_api() {
        Some(spawn_identity(&services, &mut background)?)
    } else {
        None
    };
    let worker_health = if role.runs_worker() {
        let worker = build_worker(&services)?;
        let health = worker.health();
        spawn_worker(&mut tasks, worker, &shutdown, grace);
        Some(health)
    } else {
        None
    };
    let readiness: Arc<dyn postit_api::Readiness> = Arc::new(compose::RoleReadiness {
        pool: pool.clone(),
        auth: auth.clone(),
        worker: worker_health,
    });
    let mut api_addr = None;
    if let (Some(auth), Some(api_settings), Some(listener)) = (auth, api_settings, api_listener) {
        api_addr = Some(listener.local_addr()?);
        let state = api_state(
            &services,
            api_settings,
            auth,
            Arc::clone(&readiness),
            &mut background,
        );
        let router = shared_port(
            api_router(state),
            web_router.as_ref(),
            web_listener.is_some(),
        );
        tasks.spawn(serve::serve(
            listener,
            router,
            tls.clone(),
            shutdown.clone(),
            grace,
        ));
    }
    let [web_addr, worker_addr] = spawn_listeners(
        &mut tasks,
        [
            (web_listener, web_router),
            (worker_listener, Some(probe_router(readiness))),
        ],
        tls.as_ref(),
        &shutdown,
        grace,
    )?;
    // Plain HTTP on a private port Zitadel reaches directly; never TLS, never the API router,
    // so no request logging, CORS, or auth layer wraps it.
    let [internal_addr] = spawn_listeners(
        &mut tasks,
        [(
            internal_listener,
            identity_mail_router(&settings, &clock, mailer.as_ref())?,
        )],
        None,
        &shutdown,
        grace,
    )?;

    let handle = tokio::spawn(supervise(tasks, background, pool, shutdown.clone()));
    Ok(RunningServer {
        api_addr,
        worker_addr,
        web_addr,
        internal_addr,
        shutdown,
        handle,
    })
}

/// The webhook router, when `identity_mail` is enabled (its settings are validated at load).
fn identity_mail_router(
    settings: &Settings,
    clock: &Arc<dyn postit_core::Clock>,
    mailer: Option<&Arc<dyn postit_mail::Mailer>>,
) -> anyhow::Result<Option<axum::Router>> {
    let m = &settings.identity_mail;
    if !m.enabled {
        return Ok(None);
    }
    let mailer = mailer.context("identity_mail needs the SMTP mailer")?;
    let signing_key = m
        .signing_key
        .clone()
        .context("identity_mail.signing_key is required")?;
    Ok(Some(postit_api::internal_router(
        postit_api::IdentityMailState {
            mailer: Arc::clone(mailer),
            signing_key,
            clock: Arc::clone(clock),
            tolerance: chrono::Duration::from_std(m.signature_tolerance)
                .context("identity_mail.signature_tolerance is out of range")?,
            max_body_bytes: m.max_body_bytes,
        },
    )))
}

/// Puts the web app on the API port unless `server.web.bind` gave it a listener of its own.
fn shared_port(api: axum::Router, web: Option<&axum::Router>, own_listener: bool) -> axum::Router {
    match (web, own_listener) {
        (Some(web), false) => web::dispatch(api, web.clone()),
        _ => api,
    }
}

/// Serves each `(listener, router)` pair (the web app's own port, the worker's probes) until
/// shutdown and returns the bound addresses; a pair missing either half runs nothing.
fn spawn_listeners<const N: usize>(
    tasks: &mut JoinSet<anyhow::Result<()>>,
    pairs: [(Option<TcpListener>, Option<axum::Router>); N],
    tls: Option<&axum_server::tls_rustls::RustlsConfig>,
    shutdown: &CancellationToken,
    grace: Duration,
) -> anyhow::Result<[Option<SocketAddr>; N]> {
    let mut addrs = [None; N];
    for (slot, pair) in addrs.iter_mut().zip(pairs) {
        if let (Some(listener), Some(router)) = pair {
            *slot = Some(listener.local_addr()?);
            tasks.spawn(serve::serve(
                listener,
                router,
                tls.cloned(),
                shutdown.clone(),
                grace,
            ));
        }
    }
    Ok(addrs)
}

/// What the per-role builders below share; keeps `start` under clippy's line limit.
struct Services<'a> {
    settings: &'a Settings,
    pool: &'a sqlx::PgPool,
    http: &'a reqwest::Client,
    ids: &'a Arc<dyn postit_core::IdGenerator>,
    clock: &'a Arc<dyn postit_core::Clock>,
    jobs: &'a JobQueue,
    outbox: &'a MailOutbox,
    /// The SMTP mailer, built once for the `send_email` job and the identity-mail webhook; `None`
    /// when the role runs neither.
    mailer: Option<&'a Arc<dyn postit_mail::Mailer>>,
    /// Seals and opens secrets; the `vault_rotate` job re-seals with it.
    vault: &'a postit_vault::Vault,
    /// Connects, lists, checks, and disconnects accounts.
    accounts: &'a postit_oauth::AccountService,
}

/// The job runtime: the registry of every job this release runs, and the worker that runs it.
fn build_worker(s: &Services<'_>) -> anyhow::Result<Worker> {
    let mailer = s.mailer.context("the worker role needs the SMTP mailer")?;
    let registry = compose::registry(
        s.settings,
        s.pool,
        s.ids,
        s.clock,
        s.jobs,
        s.outbox,
        s.vault,
        Arc::clone(mailer),
    )?;
    Ok(Worker::new(
        s.pool.clone(),
        &s.settings.jobs,
        registry,
        Arc::clone(s.ids),
        Arc::clone(s.clock),
    ))
}

/// Builds the authenticator and starts its background work: the JWKS prefetch (retried
/// with backoff so `/ready` flips once the `IdP` answers) and the principal-cache listener.
fn spawn_identity(
    s: &Services<'_>,
    background: &mut JoinSet<()>,
) -> anyhow::Result<Arc<dyn Authenticate>> {
    let identity = compose::identity(s.settings, s.pool, s.http, s.ids, s.clock, s.outbox)?;
    let auth: Arc<dyn Authenticate> = identity.auth.clone();
    let prefetch = Arc::clone(&auth);
    background.spawn(async move {
        let mut delay = Duration::from_secs(1);
        while let Err(err) = prefetch.prefetch().await {
            tracing::warn!(error = %err, retry_in = ?delay, "JWKS not loaded yet");
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(60));
        }
        tracing::info!("JWKS loaded");
    });
    background.spawn(postit_identity::cache::run_listener(
        s.pool.clone(),
        identity.cache.clone(),
    ));
    Ok(auth)
}

/// Runs the job worker until shutdown. The drain budget (`grace` + 5 s) starts when
/// shutdown is requested, never at startup; a worker still busy after it is dropped and its
/// in-flight jobs are left to apalis retry.
fn spawn_worker(
    tasks: &mut JoinSet<anyhow::Result<()>>,
    worker: Worker,
    shutdown: &CancellationToken,
    grace: Duration,
) {
    let stop = shutdown.clone();
    tasks.spawn(async move {
        let signal = stop.clone();
        let run = worker.run(async move { signal.cancelled().await });
        tokio::pin!(run);
        tokio::select! {
            result = &mut run => result.map_err(anyhow::Error::from),
            () = async {
                stop.cancelled().await;
                tokio::time::sleep(grace + Duration::from_secs(5)).await;
            } => {
                tracing::warn!("job worker did not drain within server.shutdown_timeout");
                Ok(())
            }
        }
    });
}

/// The API port's state, plus the minute-by-minute rate-limiter sweep (IP, user, and
/// provisioning buckets), which stops with the background set at shutdown.
fn api_state(
    s: &Services<'_>,
    api_settings: ApiSettings,
    auth: Arc<dyn Authenticate>,
    readiness: Arc<dyn postit_api::Readiness>,
    background: &mut JoinSet<()>,
) -> AppState {
    let limits = Arc::new(Limits::new(&api_settings.rate_limit));
    let sweeper = Arc::clone(&limits);
    background.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            sweeper.retain_recent();
        }
    });
    AppState {
        auth,
        admin: UserAdminService::new(
            s.pool.clone(),
            Arc::clone(s.ids),
            s.jobs.clone(),
            s.outbox.clone(),
        ),
        pool: s.pool.clone(),
        settings: Arc::new(api_settings),
        limits,
        readiness,
        console: postit_jobs::JobConsole::new(
            s.pool.clone(),
            s.jobs.clone(),
            compose::console_catalog(s.settings),
            Arc::clone(s.clock),
        ),
        clock: Arc::clone(s.clock),
        accounts: s.accounts.clone(),
    }
}

/// Waits for the serve and worker tasks; the first failure cancels the rest. Then stops
/// background work and closes the pool. The close is bounded: `PgPool::close` waits for every
/// checked-out connection, and a job abandoned at the drain deadline may still hold one.
async fn supervise(
    mut tasks: JoinSet<anyhow::Result<()>>,
    mut background: JoinSet<()>,
    pool: sqlx::PgPool,
    stop: CancellationToken,
) -> anyhow::Result<()> {
    let mut first_error = None;
    while let Some(joined) = tasks.join_next().await {
        let result = joined.map_err(anyhow::Error::from).and_then(|r| r);
        if let Err(err) = result {
            tracing::error!(error = %format!("{err:#}"), "a server task failed; shutting down");
            stop.cancel();
            first_error.get_or_insert(err);
        }
    }
    background.shutdown().await;
    if tokio::time::timeout(Duration::from_secs(5), pool.close())
        .await
        .is_err()
    {
        tracing::warn!("database pool did not close within 5s; exiting anyway");
    }
    first_error.map_or(Ok(()), Err)
}

async fn signal(shutdown: CancellationToken) {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            // Not a signal: never shut down (or exit 130) because the handler failed.
            tracing::warn!(error = %err, "cannot listen for Ctrl-C; relying on SIGTERM only");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(err) => {
                tracing::warn!(error = %err, "cannot listen for SIGTERM; relying on Ctrl-C only");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutdown signal received; draining (Ctrl-C again exits at once)");
    shutdown.cancel();
    // A second Ctrl-C forces the exit, so a native run can always be stopped.
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::warn!(error = %err, "cannot listen for a second Ctrl-C");
        std::future::pending::<()>().await;
    }
    tracing::warn!("second shutdown signal; exiting without draining");
    std::process::exit(130);
}

fn config_dir() -> PathBuf {
    std::env::var_os("POSTIT_CONFIG_DIR").map_or_else(|| PathBuf::from("config"), PathBuf::from)
}

fn environment() -> anyhow::Result<Environment> {
    Environment::from_env().context("resolving POSTIT_ENV")
}

/// `postit` with no arguments.
///
/// # Errors
///
/// Startup failures, or the first failing server task.
pub async fn run_from_env() -> anyhow::Result<()> {
    let env = environment()?;
    let role = Role::resolve(std::env::var("POSTIT_ROLE").ok().as_deref(), env)?;
    let settings = postit_config::load(env, &config_dir()).context("loading configuration")?;
    telemetry::init(env);
    let running = start(StartOptions {
        env,
        role,
        settings,
        db: None,
        api_listener: None,
        worker_listener: None,
        internal_listener: None,
    })
    .await?;
    tracing::info!(
        api = ?running.api_addr,
        worker = ?running.worker_addr,
        internal = ?running.internal_addr,
        "postit is listening"
    );
    tokio::spawn(signal(running.shutdown.clone()));
    running.handle.await.context("server task panicked")?
}

/// `postit healthcheck`.
///
/// # Errors
///
/// Unhealthy, unreachable, or TLS enabled.
pub fn healthcheck_from_env() -> anyhow::Result<()> {
    let env = environment()?;
    let role = Role::resolve(std::env::var("POSTIT_ROLE").ok().as_deref(), env)?;
    let settings = postit_config::load(env, &config_dir()).context("loading configuration")?;
    if settings.server.tls.enabled {
        anyhow::bail!("healthcheck requires plain HTTP (server.tls.enabled = true)");
    }
    let port = if role.runs_api() {
        settings.server.api_port
    } else {
        settings.server.worker_port
    };
    healthcheck::probe(SocketAddr::from(([127, 0, 0, 1], port)))
}
