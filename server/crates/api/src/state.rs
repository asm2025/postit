use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use postit_config::RateLimitSettings;
use postit_core::UserId;
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

pub struct Limits {
    pub ip: KeyedLimiter<IpAddr>,
    pub user: KeyedLimiter<UserId>,
    pub provisioning: KeyedLimiter<IpAddr>,
}

impl Limits {
    #[must_use]
    pub fn new(settings: &RateLimitSettings) -> Self {
        Self {
            ip: KeyedLimiter::new(&settings.unauthenticated),
            user: KeyedLimiter::new(&settings.authenticated),
            provisioning: KeyedLimiter::new(&settings.provisioning),
        }
    }

    pub fn retain_recent(&self) {
        self.ip.retain_recent();
        self.user.retain_recent();
        self.provisioning.retain_recent();
    }
}

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<dyn Authenticate>,
    pub admin: UserAdminService,
    pub pool: PgPool,
    pub settings: Arc<ApiSettings>,
    pub limits: Arc<Limits>,
    pub readiness: Arc<dyn Readiness>,
}
