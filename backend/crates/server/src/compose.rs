use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use postit_api::state::Readiness;
use postit_config::{Environment, Settings};
use postit_core::{AgentRegistry, Clock, IdGenerator, SystemClock, SystemIdGenerator};
use postit_data::limits::UserLimitDefaults;
use postit_identity::auth::{Authenticate, Authenticator, AuthenticatorParts};
use postit_identity::cache::PrincipalCache;
use postit_identity::claims::{ClaimsConfig, ClaimsTransformer};
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::verifier::Verifier;
use postit_jobs::{ConsoleCatalog, JobRegistry, WorkerHealth};
use postit_mail::{MailLoaders, MailOutbox, Mailer, SendEmailDeps, SendEmailHandler, SmtpMailer};
use postit_vault::Vault;
use sqlx::PgPool;

/// The platform agents this build includes. Each agent crate is registered here behind its
/// Cargo feature, one line per platform.
///
/// # Errors
///
/// Fails on an agent whose descriptor is invalid or whose platform is already registered.
pub fn agent_registry() -> anyhow::Result<AgentRegistry> {
    // Without an agent feature nothing registers, so the binding is not mutated.
    #[cfg_attr(not(feature = "mock-agent"), allow(unused_mut))]
    let mut registry = AgentRegistry::new();
    #[cfg(feature = "mock-agent")]
    registry.register(Arc::new(postit_agent_mock::MockAgent::broadcast()))?;
    #[cfg(feature = "mock-agent")]
    registry.register(Arc::new(postit_agent_mock::MockAgent::direct()))?;
    Ok(registry)
}

/// Refuses to start a production deployment whose binary carries the mock agent.
///
/// # Errors
///
/// Fails when the `mock-agent` feature is compiled in and `environment` is production.
pub fn refuse_mock_in_production(environment: Environment) -> anyhow::Result<()> {
    if cfg!(feature = "mock-agent") && environment == Environment::Production {
        anyhow::bail!(
            "the mock agent is compiled in; production builds must not enable the mock-agent feature"
        );
    }
    Ok(())
}

/// The account service over the compiled-in agents. Link signing is the "not available yet"
/// stand-in until C2 replaces it.
///
/// # Errors
///
/// Fails when the public HTTP client cannot be built.
#[allow(clippy::too_many_arguments)] // the composition root names every dependency
pub fn account_service(
    settings: &Settings,
    pool: &PgPool,
    http: &reqwest::Client,
    vault: &Vault,
    catalog: Arc<postit_oauth::PlatformCatalog>,
    media: (
        Arc<dyn postit_core::MediaStore>,
        Arc<dyn postit_core::MediaHost>,
    ),
    ids: &Arc<dyn IdGenerator>,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<postit_oauth::AccountService> {
    let public =
        postit_http::build_public_client(&settings.http, postit_http::AddressPolicy::strict())
            .context("building the public HTTP client")?;
    Ok(postit_oauth::AccountService::new(
        postit_oauth::AccountServiceDeps {
            pool: pool.clone(),
            vault: vault.clone(),
            catalog,
            http: postit_http::agent_handles(http.clone(), public),
            public_url: settings.server.public_url.clone(),
            media_store: media.0,
            media_host: media.1,
            link_signer: Arc::new(crate::unavailable::UnavailableLinks {
                base: settings.server.public_url.clone(),
            }),
            clock: Arc::clone(clock),
            ids: Arc::clone(ids),
            settings: settings.accounts.clone(),
        },
    ))
}

/// The process-wide redactor for this deployment: the built-in patterns (Slack and Discord
/// webhooks, Telegram bot token paths), plus the signed media route
/// (`{media.public_base_url}/public/media/`) and the email tracking and unsubscribe routes
/// (`{email.tracking_base_url}/public/`), each base falling back to `server.public_url`.
/// Their path segments are signed tokens, so logs show only the host. Any other host that
/// carries a secret in its path is added here: the redactor is installed once at startup,
/// before any agent exists.
#[must_use]
pub fn redactor(settings: &Settings) -> postit_http::Redactor {
    let mut redactor = postit_http::Redactor::with_defaults();
    let server = &settings.server.public_url;
    let routes = [
        (settings.media.public_base_url.as_ref(), "public/media/"),
        (settings.email.tracking_base_url.as_ref(), "public/"),
    ];
    for (base, route) in routes {
        let base = base.unwrap_or(server);
        if let Some(host) = base.host_str() {
            redactor.add(postit_http::SensitiveUrl {
                host: host.to_string(),
                host_suffix: false,
                path_prefix: Some(format!("{}/{route}", base.path().trim_end_matches('/'))),
            });
        }
    }
    redactor
}

/// # Errors
///
/// Fails on a name `jsonwebtoken` does not know, naming the config key.
pub fn algorithms(names: &[String]) -> anyhow::Result<Vec<jsonwebtoken::Algorithm>> {
    names
        .iter()
        .map(|n| {
            n.parse::<jsonwebtoken::Algorithm>()
                .with_context(|| format!("auth.oidc.accepted_algorithms: {n}"))
        })
        .collect()
}

/// The token pipeline's pieces that other parts of the process share.
pub struct Identity {
    /// Authenticates bearer tokens.
    pub auth: Arc<Authenticator<HttpJwksSource>>,
    /// Principal cache, invalidated by admin changes.
    pub cache: PrincipalCache,
}

/// The token pipeline for API roles: discovery, claims transformation, principal cache.
///
/// # Errors
///
/// Fails on an unknown `auth.oidc.accepted_algorithms` entry.
pub fn identity(
    settings: &Settings,
    pool: &PgPool,
    http: &reqwest::Client,
    ids: &Arc<dyn IdGenerator>,
    clock: &Arc<dyn Clock>,
    outbox: &MailOutbox,
) -> anyhow::Result<Identity> {
    let oidc = &settings.auth.oidc;
    let discovery = Arc::new(OidcDiscovery::new(
        HttpJwksSource::new(http.clone(), oidc.issuer.clone()),
        oidc.jwks_refresh_interval,
    ));
    let transformer = ClaimsTransformer::new(
        pool.clone(),
        Arc::clone(ids),
        Arc::clone(clock),
        http.clone(),
        Arc::clone(&discovery),
        outbox.clone(),
        ClaimsConfig {
            claim_names: oidc.claim_names.clone(),
            userinfo_mode: oidc.userinfo,
            bootstrap: settings.auth.bootstrap.clone(),
            approval_email_interval: settings.auth.approval_email_interval,
            limits: UserLimitDefaults::from_settings(settings),
        },
    );
    for client in oidc.unlisted_clients() {
        tracing::warn!(
            client,
            "auth.oidc.allowed_clients leaves out a configured client; its tokens are refused"
        );
    }
    let cache = PrincipalCache::new(settings.auth.principal_cache_ttl);
    let auth = Authenticator::new(AuthenticatorParts {
        verifier: Verifier::new(
            oidc.issuer.as_str().trim_end_matches('/'),
            oidc.audiences.clone(),
            algorithms(&oidc.accepted_algorithms)?,
            oidc.leeway,
        )
        .require_client(
            oidc.claim_names.client_id.clone(),
            oidc.effective_allowed_clients(),
        ),
        discovery,
        cache: cache.clone(),
        transformer,
        pool: pool.clone(),
    });
    Ok(Identity {
        auth: Arc::new(auth),
        cache,
    })
}

/// Every job the worker runs: identity maintenance and mail loaders, retention, job-history
/// purge, and `send_email`.
///
/// # Errors
///
/// Fails on a duplicate registration or a bad schedule.
pub(crate) fn registry(s: &crate::Services<'_>) -> anyhow::Result<JobRegistry> {
    let crate::Services {
        settings,
        pool,
        ids,
        clock,
        jobs,
        outbox,
        vault,
        mailer,
        ..
    } = *s;
    let mailer = mailer.context("the worker role needs the SMTP mailer")?;
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    postit_identity::jobs::register(
        &mut registry,
        &mut loaders,
        postit_identity::jobs::IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            clock: Arc::clone(clock),
            jobs: jobs.clone(),
            pending_ttl: settings.auth.pending_ttl,
            approval_email_interval: settings.auth.approval_email_interval,
            audit: settings.audit.clone(),
            schedules: settings.jobs.schedules.clone(),
        },
    )?;
    postit_jobs::maintenance::register_data_retention(
        &mut registry,
        pool.clone(),
        &settings.jobs,
        settings.retention.clone(),
        Arc::clone(clock),
    )?;
    postit_jobs::maintenance::register_job_history_purge(
        &mut registry,
        pool.clone(),
        &settings.jobs,
        Arc::clone(clock),
    )?;
    postit_mail::register(
        &mut registry,
        SendEmailHandler::new(SendEmailDeps {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            mailer: Arc::clone(mailer),
            loaders,
            outbox: outbox.clone(),
            app_url: settings.app.public_url.clone(),
            max_attempts: settings.mail.send_email_max_attempts,
        }),
    )?;
    crate::vault_rotate::register(&mut registry, pool.clone(), vault.clone(), Arc::clone(ids))?;
    postit_media::gc::register(
        &mut registry,
        pool.clone(),
        Arc::clone(&s.media.store),
        &settings.jobs.schedules,
        settings.media.upload_session_ttl,
        Arc::clone(clock),
    )?;
    Ok(registry.with_console(console_catalog(settings)))
}

/// The SMTP mailer for `[mail]`, shared by the `send_email` job and the identity-mail webhook.
///
/// # Errors
///
/// Fails on an SMTP config the mailer rejects.
pub fn mailer(settings: &Settings) -> anyhow::Result<Arc<dyn Mailer>> {
    let mailer = SmtpMailer::new(&settings.mail).context("building the SMTP mailer")?;
    Ok(Arc::new(mailer))
}

/// The admin console's metadata for every job type this deployment registers.
#[must_use]
pub fn console_catalog(settings: &Settings) -> ConsoleCatalog {
    let mut catalog = ConsoleCatalog::default();
    postit_jobs::maintenance::console(&mut catalog, &settings.jobs);
    postit_identity::jobs::console(&mut catalog, &settings.jobs.schedules);
    postit_mail::console(&mut catalog);
    crate::vault_rotate::console(&mut catalog);
    postit_media::gc::console(&mut catalog, &settings.jobs.schedules);
    catalog
}

/// `/ready` for the role: the database always, the JWKS for API roles, the worker for
/// worker roles. Role `all` serves this same check on both ports.
pub struct RoleReadiness {
    /// Database pool probed by the check.
    pub pool: PgPool,
    /// Authenticator whose signing keys must be loaded, for API roles.
    pub auth: Option<Arc<dyn Authenticate>>,
    /// Worker liveness, for worker roles.
    pub worker: Option<WorkerHealth>,
}

#[async_trait]
impl Readiness for RoleReadiness {
    async fn check(&self) -> Result<(), &'static str> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|_| "database")?;
        if self.auth.as_ref().is_some_and(|a| !a.jwks_ready()) {
            return Err("jwks");
        }
        if self.worker.as_ref().is_some_and(|w| !w.is_ready()) {
            return Err("worker");
        }
        Ok(())
    }
}

/// The production id generator.
#[must_use]
pub fn system_ids() -> Arc<dyn IdGenerator> {
    Arc::new(SystemIdGenerator)
}

/// The wall clock, built once by the composition root and handed to every service.
#[must_use]
pub fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

/// What the media service is built from, and the two views of it other services take.
pub struct Media {
    /// Uploads, serves, signs, and deletes media.
    pub service: postit_media::MediaService,
    /// Reads stored files; agents stream media through it.
    pub store: Arc<postit_media::FsMediaStore>,
    /// Signs public URLs for platforms that fetch media themselves.
    pub host: Arc<postit_media::SignedUrlMediaHost>,
}

/// Builds the media service: the storage directory, the probe, the thumbnailer, and the URL
/// signer.
///
/// # Errors
///
/// Fails naming `media.signing_key` when it is unset, and naming `media.storage_root` when the
/// directory cannot be created or written.
pub fn media_service(
    settings: &Settings,
    pool: &PgPool,
    ids: &Arc<dyn IdGenerator>,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<Media> {
    let m = &settings.media;
    let key = m
        .signing_key
        .as_ref()
        .context("media.signing_key is required (set POSTIT__MEDIA__SIGNING_KEY)")?;
    let store = Arc::new(
        postit_media::FsMediaStore::open_root(&m.storage_root, pool.clone()).with_context(
            || {
                format!(
                    "media.storage_root {} is not usable",
                    m.storage_root.display()
                )
            },
        )?,
    );
    // A writable directory is part of the contract: fail at startup, not on the first upload.
    let check = store.root().join(".write-check");
    std::fs::write(&check, b"ok")
        .and_then(|()| std::fs::remove_file(&check))
        .with_context(|| {
            format!(
                "media.storage_root {} is not writable",
                m.storage_root.display()
            )
        })?;
    let base = m
        .public_base_url
        .clone()
        .unwrap_or_else(|| settings.server.public_url.clone());
    let signer = Arc::new(postit_media::UrlSigner::new(key.expose().as_bytes(), base));
    #[cfg(feature = "ffprobe")]
    let probe: Arc<dyn postit_core::MediaProbe> =
        Arc::new(postit_media::FfprobeProbe::new(m.ffprobe_path.clone()));
    #[cfg(not(feature = "ffprobe"))]
    let probe: Arc<dyn postit_core::MediaProbe> = Arc::new(postit_media::NativeProbe);
    let host = Arc::new(postit_media::SignedUrlMediaHost::new(
        Arc::clone(&signer),
        Arc::clone(clock),
    ));
    let service = postit_media::MediaService::new(postit_media::MediaServiceDeps {
        pool: pool.clone(),
        store: Arc::clone(&store),
        probe,
        thumbnailer: postit_media::Thumbnailer::new(m.thumbnail_max_pixels, m.ffmpeg_path.clone()),
        signer,
        clock: Arc::clone(clock),
        ids: Arc::clone(ids),
        settings: m.clone(),
    });
    Ok(Media {
        service,
        store,
        host,
    })
}
