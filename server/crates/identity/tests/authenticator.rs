use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use postit_config::BootstrapSettings;
use postit_data::users::{UserStatus, UsersRepo};
use postit_identity::VerifyError;
use postit_identity::auth::{AllowAll, AuthError, Authenticate, ProvisionGate, RateLimited};
use postit_identity::cache::PrincipalCache;
use postit_identity::testkit::{TestIssuer, authenticator, authenticator_with_cache};
use secrecy::SecretString;
use sqlx::PgPool;

fn no_bootstrap() -> BootstrapSettings {
    BootstrapSettings {
        admin_email: None,
        admin_subject: None,
    }
}

fn bearer(issuer: &TestIssuer, sub: &str) -> SecretString {
    let (token, _) = issuer.token_and_claims(
        sub,
        Some(&format!("{sub}@postit.test")),
        Some(true),
        Some(sub),
    );
    SecretString::from(token)
}

async fn user_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

struct CountingGate {
    calls: AtomicUsize,
    deny: bool,
}

impl ProvisionGate for CountingGate {
    fn check(&self) -> Result<(), RateLimited> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.deny {
            Err(RateLimited {
                retry_after: Duration::from_secs(7),
            })
        } else {
            Ok(())
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn first_request_provisions_once_and_sets_last_seen(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");

    let principal = auth
        .authenticate(&token, &AllowAll)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(principal.status, UserStatus::Pending);
    let again = auth
        .authenticate(&token, &AllowAll)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(again.user_id, principal.user_id);
    assert_eq!(user_count(&pool).await, 1);

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let row = UsersRepo::find_by_id(&mut conn, principal.user_id)
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("row exists"));
    assert!(row.last_seen_at.is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_cache_hit_skips_the_database(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");
    auth.authenticate(&token, &AllowAll)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));

    // Close the pool: a second authenticate must be served from the cache alone.
    pool.close().await;
    let cached = auth.authenticate(&token, &AllowAll).await;
    assert!(cached.is_ok(), "got {cached:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_gate_runs_only_when_a_row_would_be_created(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let auth = authenticator_with_cache(pool.clone(), &issuer, no_bootstrap(), cache.clone());
    let gate = CountingGate {
        calls: AtomicUsize::new(0),
        deny: false,
    };
    let token = bearer(&issuer, "alice");

    auth.authenticate(&token, &gate)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);

    cache.invalidate_all(); // force a miss for an existing user
    auth.authenticate(&token, &gate)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(
        gate.calls.load(Ordering::SeqCst),
        1,
        "existing user must not hit the gate"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_gate_denial_creates_nothing(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let gate = CountingGate {
        calls: AtomicUsize::new(0),
        deny: true,
    };

    let result = auth.authenticate(&bearer(&issuer, "alice"), &gate).await;
    assert!(
        matches!(result, Err(AuthError::RateLimited(r)) if r.retry_after == Duration::from_secs(7))
    );
    assert_eq!(user_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_invalid_token_never_touches_the_database(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let other = TestIssuer::start().await; // different keys, same kid naming scheme
    let foreign = bearer(&other, "mallory");

    let result = auth.authenticate(&foreign, &AllowAll).await;
    assert!(
        matches!(result, Err(AuthError::Invalid(_))),
        "got {result:?}"
    );
    assert_eq!(user_count(&pool).await, 0);

    let garbage = auth
        .authenticate(&SecretString::from("not.a.jwt".to_string()), &AllowAll)
        .await;
    assert!(
        matches!(garbage, Err(AuthError::Invalid(VerifyError::Malformed))),
        "got {garbage:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn jwks_unavailable_before_the_issuer_answers(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let token = bearer(&issuer, "alice");
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    // The IdP is down before anything was cached. Keep `issuer` alive: a dropped wiremock
    // server returns to a shared pool and a parallel test may mount a working issuer on it.
    issuer.fail_all().await;
    assert!(!auth.jwks_ready());
    let result = auth.authenticate(&token, &AllowAll).await;
    assert!(
        matches!(result, Err(AuthError::JwksUnavailable)),
        "got {result:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn prefetch_makes_jwks_ready(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool, &issuer, no_bootstrap());
    assert!(!auth.jwks_ready());
    auth.prefetch()
        .await
        .unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    assert!(auth.jwks_ready());
    assert!(auth.discovery_document().is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalidate_user_forces_a_fresh_read(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");
    let first = auth
        .authenticate(&token, &AllowAll)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::set_status(&mut conn, first.user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));
    auth.invalidate_user(first.user_id);
    let fresh = auth
        .authenticate(&token, &AllowAll)
        .await
        .unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(fresh.status, UserStatus::Active);
}
