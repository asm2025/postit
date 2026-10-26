//! Shared handler state.

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use postit_config::RateLimitSettings;
use postit_core::{Clock, UserId};
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use sqlx::PgPool;

use crate::limit::KeyedLimiter;
use crate::settings::ApiSettings;

/// What `/ready` asks. `postit-server` composes one per role.
#[async_trait]
pub trait Readiness: Send + Sync {
    /// # Errors
    ///
    /// Returns the name of the first failing check (`database`, `jwks`, `worker`).
    async fn check(&self) -> Result<(), &'static str>;
}

/// The rate limiters, one per request class.
pub struct Limits {
    /// Per client IP, for requests that do not authenticate.
    pub ip: KeyedLimiter<IpAddr>,
    /// Per user, for authenticated requests.
    pub user: KeyedLimiter<UserId>,
    /// Per client IP, for first sign-in provisioning.
    pub provisioning: KeyedLimiter<IpAddr>,
    /// Per user, for requests that check a pasted credential against a platform.
    pub credential_checks: KeyedLimiter<UserId>,
}

impl Limits {
    /// Limiters sized by `settings`.
    #[must_use]
    pub fn new(settings: &RateLimitSettings) -> Self {
        Self {
            ip: KeyedLimiter::new(&settings.unauthenticated),
            user: KeyedLimiter::new(&settings.authenticated),
            provisioning: KeyedLimiter::new(&settings.provisioning),
            credential_checks: KeyedLimiter::new(&settings.credential_checks),
        }
    }

    /// Forgets keys that have recovered, bounding memory.
    pub fn retain_recent(&self) {
        self.ip.retain_recent();
        self.user.retain_recent();
        self.provisioning.retain_recent();
        self.credential_checks.retain_recent();
    }
}

/// Shared state of every handler.
#[derive(Clone)]
pub struct AppState {
    /// Authenticates bearer tokens.
    pub auth: Arc<dyn Authenticate>,
    /// User administration.
    pub admin: UserAdminService,
    /// Database pool.
    pub pool: PgPool,
    /// HTTP-layer settings.
    pub settings: Arc<ApiSettings>,
    /// Rate limiters.
    pub limits: Arc<Limits>,
    /// Readiness checks behind `/ready`.
    pub readiness: Arc<dyn Readiness>,
    /// The admin job console.
    pub console: postit_jobs::JobConsole,
    /// Source of time.
    pub clock: Arc<dyn Clock>,
    /// Connecting, listing, checking, and disconnecting accounts.
    pub accounts: postit_oauth::AccountService,
    /// Uploads, serves, signs, and deletes media.
    pub media: postit_media::MediaService,
}
