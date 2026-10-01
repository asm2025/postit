use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
    generation: Arc<AtomicU64>,
}

impl PrincipalCache {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            identity_index: Cache::builder().time_to_live(ttl).build(),
            principals: Cache::builder().time_to_live(ttl).build(),
            generation: Arc::new(AtomicU64::new(0)),
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

    /// Snapshot taken before a cache-miss database read; see [`Self::insert_if_current`].
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Inserts only if no invalidation happened since `generation` was read, so a principal
    /// loaded before a concurrent admin change can't be cached after that change's eviction.
    /// A single global counter is coarse (any eviction discards concurrent inserts, which
    /// just become the next request's miss) but correct and allocation-free.
    #[must_use]
    pub fn insert_if_current(
        &self,
        generation: u64,
        issuer: &str,
        subject: &str,
        principal: Principal,
    ) -> bool {
        if self.generation() != generation {
            return false;
        }
        let user_id = principal.user_id;
        self.insert(issuer, subject, principal);
        // Re-check: an invalidation between the check and the insert must win.
        if self.generation() != generation {
            self.principals.invalidate(&user_id);
            return false;
        }
        true
    }

    pub fn invalidate_user(&self, user_id: UserId) {
        // Bumped before evicting: a reader either sees the new generation and backs out, or
        // inserted before the bump and is removed by the eviction that follows.
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.principals.invalidate(&user_id);
    }

    pub fn invalidate_all(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
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
///
/// Uses `try_recv` rather than `recv`: `PgListener` transparently reconnects and
/// re-`LISTEN`s on most connection losses, so `recv` would never surface an ordinary
/// network drop as an `Err`. `try_recv` instead returns `Ok(None)` when it had to
/// reconnect, which is treated the same as a hard failure — `cache.invalidate_all()` — so a
/// notification sent while the listener was down isn't silently missed. `invalidate_all()`
/// also runs once right after the initial `listen()` succeeds, closing the gap where a
/// `NOTIFY` sent while nothing was listening (between sessions, or before this one
/// connected) would otherwise be lost rather than merely delayed.
pub async fn run_one_listen_session(pool: PgPool, cache: PrincipalCache) {
    let outcome: Result<(), sqlx::Error> = async {
        let mut listener = PgListener::connect_with(&pool).await?;
        sqlx::query("SELECT set_config('application_name', $1, false)")
            .bind(LISTENER_APPLICATION_NAME)
            .execute(&mut listener)
            .await?;
        listener.listen(CHANNEL).await?;
        cache.invalidate_all();

        loop {
            match listener.try_recv().await? {
                Some(notification) => {
                    if let Ok(uuid) = uuid::Uuid::parse_str(notification.payload()) {
                        cache.invalidate_user(UserId::from(uuid));
                    }
                }
                None => {
                    // Transparent reconnect: a NOTIFY sent during the gap is gone for
                    // good, so fall back to clearing everything rather than silently
                    // continuing as if nothing happened.
                    cache.invalidate_all();
                }
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
