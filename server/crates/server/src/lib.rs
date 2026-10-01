//! The composition root (plan 02, `postit-server`). `main.rs` calls [`run_from_env`];
//! tests call [`start`] with their own pool and ephemeral listeners.

pub mod compose;
pub mod healthcheck;
pub mod role;
pub mod serve;
pub mod telemetry;

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

pub struct StartOptions {
    pub env: Environment,
    pub role: Role,
    pub settings: Settings,
    /// Use this pool instead of connecting from `settings.database` (tests).
    pub db: Option<postit_data::Db>,
    pub api_listener: Option<TcpListener>,
    pub worker_listener: Option<TcpListener>,
}

pub struct RunningServer {
    pub api_addr: Option<SocketAddr>,
    pub worker_addr: Option<SocketAddr>,
    pub shutdown: CancellationToken,
    pub handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

fn bind(host: &str, port: u16) -> anyhow::Result<TcpListener> {
    TcpListener::bind((host, port)).with_context(|| format!("binding {host}:{port}"))
}

/// Connects (unless a pool was injected), migrates both schemas, and runs the bootstrap check.
async fn open_database(
    env: Environment,
    settings: &Settings,
    db: Option<postit_data::Db>,
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
    Ok(pool)
}

/// Everything that can be rejected before the database is touched or any task starts.
struct Prepared {
    api_settings: Option<ApiSettings>,
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
    api_listener: Option<TcpListener>,
    worker_listener: Option<TcpListener>,
}

/// Validates the settings the roles need (CORS origins, trusted proxies, TLS files) and binds
/// the ports, so a bad config or a taken port fails startup with nothing running.
async fn prepare(
    env: Environment,
    role: Role,
    settings: &Settings,
    api_listener: Option<TcpListener>,
    worker_listener: Option<TcpListener>,
) -> anyhow::Result<Prepared> {
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
    Ok(Prepared {
        api_settings,
        tls,
        api_listener,
        worker_listener,
    })
}

/// Startup per plan 02: validate, migrate, identity, bootstrap check, services, job registry, serve.
///
/// # Errors
///
/// Any startup failure; nothing is left running when this returns `Err`.
pub async fn start(opts: StartOptions) -> anyhow::Result<RunningServer> {
    let StartOptions {
        env,
        role,
        settings,
        db,
        api_listener,
        worker_listener,
    } = opts;
    if let Ok(dump) = settings.redacted_dump() {
        tracing::info!(config = %dump, role = ?role, "starting postit");
    }
    if settings.server.web.enabled {
        tracing::warn!(
            "server.web.enabled is set; serving the web client arrives in plan 02 P7 and is ignored"
        );
    }
    let Prepared {
        api_settings,
        tls,
        api_listener,
        worker_listener,
    } = prepare(env, role, &settings, api_listener, worker_listener).await?;

    let pool = open_database(env, &settings, db).await?;

    let http = postit_http::build_client(&settings.http).context("building the HTTP client")?;
    let ids = compose::system_ids();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
    let outbox = MailOutbox::new(jobs.clone());
    let shutdown = CancellationToken::new();
    let grace = settings.server.shutdown_timeout;

    let mut tasks: JoinSet<anyhow::Result<()>> = JoinSet::new();
    let mut background: JoinSet<()> = JoinSet::new();
    let services = Services {
        settings: &settings,
        pool: &pool,
        http: &http,
        ids: &ids,
        jobs: &jobs,
        outbox: &outbox,
    };

    let auth = if role.runs_api() {
        Some(spawn_identity(&services, &mut background)?)
    } else {
        None
    };
    let worker_health = if role.runs_worker() {
        let registry = compose::registry(&settings, &pool, &ids, &jobs, &outbox)?;
        let worker = Worker::new(pool.clone(), &settings.jobs, registry);
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
        tasks.spawn(serve::serve(
            listener,
            api_router(state),
            tls.clone(),
            shutdown.clone(),
            grace,
        ));
    }
    let mut worker_addr = None;
    if let Some(listener) = worker_listener {
        worker_addr = Some(listener.local_addr()?);
        tasks.spawn(serve::serve(
            listener,
            probe_router(readiness),
            tls,
            shutdown.clone(),
            grace,
        ));
    }

    let handle = tokio::spawn(supervise(tasks, background, pool, shutdown.clone()));
    Ok(RunningServer {
        api_addr,
        worker_addr,
        shutdown,
        handle,
    })
}

/// What the per-role builders below share; keeps `start` under clippy's line limit.
struct Services<'a> {
    settings: &'a Settings,
    pool: &'a sqlx::PgPool,
    http: &'a reqwest::Client,
    ids: &'a Arc<dyn postit_core::IdGenerator>,
    jobs: &'a JobQueue,
    outbox: &'a MailOutbox,
}

/// Builds the authenticator and starts its background work: the JWKS prefetch (retried
/// with backoff so `/ready` flips once the `IdP` answers) and the principal-cache listener.
fn spawn_identity(
    s: &Services<'_>,
    background: &mut JoinSet<()>,
) -> anyhow::Result<Arc<dyn Authenticate>> {
    let identity = compose::identity(s.settings, s.pool, s.http, s.ids, s.outbox)?;
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
    })
    .await?;
    tracing::info!(api = ?running.api_addr, worker = ?running.worker_addr, "postit is listening");
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
