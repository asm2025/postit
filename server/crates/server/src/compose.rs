use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use postit_api::state::Readiness;
use postit_config::Settings;
use postit_core::{IdGenerator, SystemIdGenerator};
use postit_identity::auth::{Authenticate, Authenticator, AuthenticatorParts};
use postit_identity::cache::PrincipalCache;
use postit_identity::claims::{ClaimsConfig, ClaimsTransformer};
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::verifier::Verifier;
use postit_jobs::{JobQueue, JobRegistry, WorkerHealth};
use postit_mail::{MailLoaders, MailOutbox, SendEmailDeps, SendEmailHandler, SmtpMailer};
use sqlx::PgPool;

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

pub struct Identity {
    pub auth: Arc<Authenticator<HttpJwksSource>>,
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
    let cache = PrincipalCache::new(settings.auth.principal_cache_ttl);
    let auth = Authenticator::new(AuthenticatorParts {
        verifier: Verifier::new(
            oidc.issuer.as_str().trim_end_matches('/'),
            oidc.audiences.clone(),
            algorithms(&oidc.accepted_algorithms)?,
            oidc.leeway,
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
    jobs: &JobQueue,
    outbox: &MailOutbox,
) -> anyhow::Result<JobRegistry> {
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    postit_identity::jobs::register(
        &mut registry,
        &mut loaders,
        postit_identity::jobs::IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            jobs: jobs.clone(),
            pending_ttl: settings.auth.pending_ttl,
            approval_email_interval: settings.auth.approval_email_interval,
            audit: settings.audit.clone(),
            schedules: settings.jobs.schedules.clone(),
        },
    )?;
    postit_jobs::maintenance::register_data_retention(&mut registry, pool.clone(), &settings.jobs)?;
    postit_jobs::maintenance::register_job_history_purge(
        &mut registry,
        pool.clone(),
        &settings.jobs,
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
    Ok(registry)
}

/// `/ready` for the role: the database always, the JWKS for API roles, the worker for
/// worker roles. Role `all` serves this same check on both ports.
pub struct RoleReadiness {
    pub pool: PgPool,
    pub auth: Option<Arc<dyn Authenticate>>,
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

#[must_use]
pub fn system_ids() -> Arc<dyn IdGenerator> {
    Arc::new(SystemIdGenerator)
}
