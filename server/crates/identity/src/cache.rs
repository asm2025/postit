use std::time::Duration;

use moka::sync::Cache;
use postit_core::UserId;
use sqlx::PgPool;
use sqlx::postgres::PgListener;

use crate::principal::Principal;

/// The `application_name` this crate's `LISTEN` connection sets on itself, so an operator
/// (or a test) can find and, if needed, terminate that specific backend through
/// `pg_stat_activity` without guessing which connection in the pool it is.
pub const LISTENER_APPLICATION_NAME: &str = "postit_principal_cache_listener";

const CHANNEL: &str = "postit_user_changed";

/// Two small caches, both TTL `auth.principal_cache_ttl`: `(iss, sub) -> UserId` and
/// `UserId -> Principal`. Claims transformation looks up the first then the second; the
/// `LISTEN` task evicts the second directly by the user id in its notification payload, no
/// reverse lookup needed. See the design spec for why this is two caches, not one.
#[derive(Clone)]
pub struct PrincipalCache {
    identity_index: Cache<(String, String), UserId>,
    principals: Cache<UserId, Principal>,
}

impl PrincipalCache {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            identity_index: Cache::builder().time_to_live(ttl).build(),
            principals: Cache::builder().time_to_live(ttl).build(),
        }
    }

    #[must_use]
    pub fn get(&self, issuer: &str, subject: &str) -> Option<Principal> {
        let user_id = self
            .identity_index
            .get(&(issuer.to_string(), subject.to_string()))?;
        self.principals.get(&user_id)
    }

    pub fn insert(&self, issuer: &str, subject: &str, principal: Principal) {
        self.identity_index
            .insert((issuer.to_string(), subject.to_string()), principal.user_id);
        self.principals.insert(principal.user_id, principal);
    }

    pub fn invalidate_user(&self, user_id: UserId) {
        self.principals.invalidate(&user_id);
    }

    pub fn invalidate_all(&self) {
        self.principals.invalidate_all();
    }
}

/// Runs one `LISTEN` session to completion: connects, sets
/// [`LISTENER_APPLICATION_NAME`], listens on `postit_user_changed`, and evicts the
/// notified user from `cache` on every notification. Returns once the connection fails for
/// any reason (including never having connected at all), first calling
/// `cache.invalidate_all()` — so a caller never needs to distinguish "never listened" from
/// "was listening, then the connection dropped": both end the same way. `postit-server`
/// (P6) is the only production caller, through [`run_listener`]; this function stays
/// separate so a test can await one session's natural end instead of managing an infinite
/// loop.
pub async fn run_one_listen_session(pool: PgPool, cache: PrincipalCache) {
    let outcome: Result<(), sqlx::Error> = async {
        let mut listener = PgListener::connect_with(&pool).await?;
        sqlx::query("SET application_name = $1")
            .bind(LISTENER_APPLICATION_NAME)
            .execute(&mut listener)
            .await?;
        listener.listen(CHANNEL).await?;

        loop {
            let notification = listener.recv().await?;
            if let Ok(uuid) = uuid::Uuid::parse_str(notification.payload()) {
                cache.invalidate_user(UserId::from(uuid));
            }
        }
    }
    .await;

    if outcome.is_err() {
        cache.invalidate_all();
    }
}

/// The production entry point: runs [`run_one_listen_session`] forever, with a short pause
/// between attempts so a persistently unreachable database doesn't spin. `postit-server`
/// spawns this once at startup in every process that runs the API (the worker doesn't need
/// the JWKS or the principal cache).
pub async fn run_listener(pool: PgPool, cache: PrincipalCache) {
    loop {
        run_one_listen_session(pool.clone(), cache.clone()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
