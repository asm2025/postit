#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};

use common::{connect_all, force_state, found, job_for, mock_agent, platforms};
use postit_agent_mock::{MockScript, VerifyScript};
use postit_core::{AccountId, DelegationEvent, Notice, UserId};
use postit_data::audit::AuditContext;
use postit_data::publishing::{CancelOutcome, CancelResult};
use postit_data::testkit::{callers, grant, seed_user};
use postit_data::{AccountLimit, Level};
use postit_data::{OwnerScope, ScopeError};
use postit_notify::notifications_for;
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountError, AccountService, DisconnectHook, NativeCleanup};
use sqlx::{PgConnection, PgPool};

struct Fixture {
    service: AccountService,
    script: MockScript,
    owner: UserId,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool, accounts: &[&str]) -> Fixture {
    platforms(pool, &["fake"]).await;
    let (agent, script) = mock_agent("fake");
    script.set_verify(VerifyScript::Accounts(
        accounts.iter().map(|id| found(id)).collect(),
    ));
    let service = account_service(pool.clone(), vec![agent], test_accounts_settings());
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_cancels_pending_and_parked_and_keeps_row(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let job = job_for(&pool, f.owner, &accounts[0]).await;
    assert_eq!(job.deliveries.len(), 1);

    let outcome = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(outcome.cancelled_deliveries, 1);
    assert_eq!(outcome.native_scheduled_remaining, 0);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM deliveries WHERE state = 'cancelled'"
        )
        .await,
        1
    );
    let kept: String = sqlx::query_scalar("SELECT status FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, "disconnected");
    let events = count(
        &pool,
        "SELECT count(*) FROM audit_events WHERE kind = 'account_disconnected'",
    )
    .await;
    assert_eq!(events, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_refused_while_in_flight(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    job_for(&pool, f.owner, &accounts[0]).await;
    force_state(&pool, accounts[0].id, "claimed").await;

    let result = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::JobActive)));
    let status: String = sqlx::query_scalar("SELECT status FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_frees_remote_key_for_another_owner(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();
    let other = OwnerScope::own(seed_user(&pool, "active").await);

    let theirs = connect_all(&f.service, &other, "fake").await;

    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].remote_account_id, "a");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn last_account_revokes_and_deletes_grant(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;

    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(f.script.revoke_calls(), 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn one_of_two_accounts_keeps_grant(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;

    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(f.script.revoke_calls(), 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_failure_still_deletes_grant(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    f.script.fail_revoke(true);

    let outcome = f
        .service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(outcome.is_ok());
    assert_eq!(f.script.revoke_calls(), 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM account_grants").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnect_owner_only(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let who = callers(&pool, f.owner, &[accounts[0].id]).await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .disconnect(scope, accounts[0].id, AuditContext::default())
            .await;
        assert!(matches!(
            result,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
    assert_eq!(f.script.revoke_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stranger_gets_not_found(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);

    let result = f
        .service
        .disconnect(&stranger, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::NotFound)));
}

/// What the recording hook saw.
#[derive(Debug, Default)]
struct Seen {
    /// Each `before_disconnect` call, with the account's status and grant count at the time.
    before: Vec<(UserId, AccountId, String, i64)>,
    /// Each in-transaction call, with the outcomes it was handed.
    cancelled: Vec<Vec<CancelOutcome>>,
}

struct RecordingHook {
    pool: PgPool,
    cleanup: NativeCleanup,
    seen: Arc<Mutex<Seen>>,
}

#[async_trait::async_trait]
impl DisconnectHook for RecordingHook {
    async fn before_disconnect(
        &self,
        owner: UserId,
        account: AccountId,
    ) -> Result<NativeCleanup, AccountError> {
        let status: String =
            sqlx::query_scalar("SELECT status FROM connected_accounts WHERE id = $1")
                .bind(account.as_uuid())
                .fetch_one(&self.pool)
                .await
                .unwrap();
        let grants = count(&self.pool, "SELECT count(*) FROM account_grants").await;
        self.seen
            .lock()
            .unwrap()
            .before
            .push((owner, account, status, grants));
        Ok(self.cleanup)
    }

    async fn deliveries_cancelled(
        &self,
        _tx: &mut PgConnection,
        _owner: UserId,
        cancelled: &[CancelResult],
    ) -> Result<(), AccountError> {
        self.seen
            .lock()
            .unwrap()
            .cancelled
            .push(cancelled.iter().map(|c| c.outcome).collect());
        Ok(())
    }
}

fn hooked(
    f: &Fixture,
    pool: &PgPool,
    cleanup: NativeCleanup,
) -> (AccountService, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let service = f.service.with_disconnect_hook(Arc::new(RecordingHook {
        pool: pool.clone(),
        cleanup,
        seen: Arc::clone(&seen),
    }));
    (service, seen)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn hook_runs_while_the_credentials_are_live_and_reports_remaining(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    job_for(&pool, f.owner, &accounts[0]).await;
    let (service, seen) = hooked(
        &f,
        &pool,
        NativeCleanup {
            deleted: 3,
            remaining: 2,
        },
    );

    let outcome = service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(outcome.native_scheduled_remaining, 2);
    assert_eq!(outcome.cancelled_deliveries, 1);
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.before,
        vec![(f.owner, accounts[0].id, "active".to_owned(), 1)]
    );
    assert_eq!(seen.cancelled, vec![vec![CancelOutcome::Cancelled]]);
    assert_eq!(f.script.revoke_calls(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn refused_while_in_flight_never_calls_the_hook(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    job_for(&pool, f.owner, &accounts[0]).await;
    force_state(&pool, accounts[0].id, "in_flight").await;
    let (service, seen) = hooked(&f, &pool, NativeCleanup::default());

    let result = service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await;

    assert!(matches!(result, Err(AccountError::JobActive)));
    let seen = seen.lock().unwrap();
    assert!(seen.before.is_empty());
    assert!(seen.cancelled.is_empty());
}

async fn rename(pool: &PgPool, user: UserId, name: &str) {
    sqlx::query("UPDATE users SET display_name = $2 WHERE id = $1")
        .bind(user.as_uuid())
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
}

/// The delegation events and party names `user` was told about.
async fn delegation_notices(pool: &PgPool, user: UserId) -> Vec<(DelegationEvent, String)> {
    let mut conn = pool.acquire().await.unwrap();
    notifications_for(&mut conn, user)
        .await
        .into_iter()
        .filter_map(|row| match row.notice {
            Notice::DelegationChanged(n) => Some((n.event, n.party_name)),
            _ => None,
        })
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn emptied_limit_notifies_both(pool: PgPool) {
    let f = fixture(&pool, &["a", "b"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let (a, b) = (accounts[0].id, accounts[1].id);
    let only_a = seed_user(&pool, "active").await;
    let a_and_b = seed_user(&pool, "active").await;
    let everything = seed_user(&pool, "active").await;
    rename(&pool, f.owner, "Olivia").await;
    rename(&pool, only_a, "Dan").await;
    for (delegate, limit) in [
        (only_a, AccountLimit::only([a])),
        (a_and_b, AccountLimit::only([a, b])),
        (everything, AccountLimit::All),
    ] {
        grant(&pool, f.owner, delegate, Level::Edit, limit).await;
    }

    f.service
        .disconnect(&f.scope, a, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(
        delegation_notices(&pool, only_a).await,
        vec![(DelegationEvent::LimitEmptied, "Olivia".to_owned())]
    );
    assert_eq!(
        delegation_notices(&pool, f.owner).await,
        vec![(DelegationEvent::LimitEmptied, "Dan".to_owned())],
        "one notice: only the grant that became empty"
    );
    assert!(delegation_notices(&pool, a_and_b).await.is_empty());
    assert!(delegation_notices(&pool, everything).await.is_empty());

    // Disconnecting the other account empties the second grant; the first, already empty,
    // is not announced again.
    f.service
        .disconnect(&f.scope, b, AuditContext::default())
        .await
        .unwrap();
    assert_eq!(delegation_notices(&pool, a_and_b).await.len(), 1);
    assert_eq!(delegation_notices(&pool, only_a).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn ended_limited_grant_is_not_announced_on_disconnect(pool: PgPool) {
    let f = fixture(&pool, &["a"]).await;
    let accounts = connect_all(&f.service, &f.scope, "fake").await;
    let delegate = seed_user(&pool, "active").await;
    let id = grant(
        &pool,
        f.owner,
        delegate,
        Level::View,
        AccountLimit::only([accounts[0].id]),
    )
    .await;
    sqlx::query("UPDATE delegations SET ended_at = now(), ended_reason = 'left' WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    f.service
        .disconnect(&f.scope, accounts[0].id, AuditContext::default())
        .await
        .unwrap();

    assert!(delegation_notices(&pool, delegate).await.is_empty());
    assert!(delegation_notices(&pool, f.owner).await.is_empty());
}
