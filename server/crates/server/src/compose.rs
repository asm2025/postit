use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use postit_api::state::Readiness;
use postit_config::Settings;
use postit_core::{AgentRegistry, Clock, IdGenerator, SystemClock, SystemIdGenerator};
use postit_identity::auth::{Authenticate, Authenticator, AuthenticatorParts};
use postit_identity::cache::PrincipalCache;
use postit_identity::claims::{ClaimsConfig, ClaimsTransformer};
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::verifier::Verifier;
use postit_jobs::{ConsoleCatalog, JobQueue, JobRegistry, WorkerHealth};
use postit_mail::{MailLoaders, MailOutbox, SendEmailDeps, SendEmailHandler, SmtpMailer};
use postit_vault::Vault;
use sqlx::PgPool;

/// The platform agents this build includes. Each agent crate is registered here behind its
/// Cargo feature, one line per platform.
///
/// # Errors
///
/// Fails on an agent whose descriptor is invalid or whose platform is already registered.
pub fn agent_registry() -> anyhow::Result<AgentRegistry> {
    // B5 registers the mock agent here behind a feature; until then the build has no agents.
    let registry = AgentRegistry::new();
    Ok(registry)
}

/// The process-wide redactor for this deployment: the built-in webhook patterns, plus the
/// signed media route (`{media.public_base_url}/public/media/`) and the email tracking and
/// unsubscribe routes (`{email.tracking_base_url}/public/`), each base falling back to
/// `server.public_url`. Their path segments are signed tokens, so logs show only the host.
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
/// Fails on a duplicate registration, a bad schedule, or an SMTP config the mailer rejects.
pub fn registry(
    settings: &Settings,
    pool: &PgPool,
    ids: &Arc<dyn IdGenerator>,
    clock: &Arc<dyn Clock>,
    jobs: &JobQueue,
    outbox: &MailOutbox,
    vault: &Vault,
) -> anyhow::Result<JobRegistry> {
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
        Arc::clone(clock),
    )?;
    postit_jobs::maintenance::register_job_history_purge(
        &mut registry,
        pool.clone(),
        &settings.jobs,
        Arc::clone(clock),
    )?;
    let mailer = SmtpMailer::new(&settings.mail).context("building the SMTP mailer")?;
    postit_mail::register(
        &mut registry,
        SendEmailHandler::new(SendEmailDeps {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            mailer: Arc::new(mailer),
            loaders,
            outbox: outbox.clone(),
            app_url: settings.app.public_url.clone(),
            max_attempts: settings.mail.send_email_max_attempts,
        }),
    )?;
    crate::vault_rotate::register(&mut registry, pool.clone(), vault.clone(), Arc::clone(ids))?;
    Ok(registry.with_console(console_catalog(settings)))
}

/// The admin console's metadata for every job type this deployment registers.
#[must_use]
pub fn console_catalog(settings: &Settings) -> ConsoleCatalog {
    let mut catalog = ConsoleCatalog::default();
    postit_jobs::maintenance::console(&mut catalog, &settings.jobs);
    postit_identity::jobs::console(&mut catalog, &settings.jobs.schedules);
    postit_mail::console(&mut catalog);
    crate::vault_rotate::console(&mut catalog);
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
