use std::sync::Arc;

use chrono::{DateTime, Duration, SecondsFormat, SubsecRound, Utc};
use emixdb::dto::Pagination;
use postit_core::{AccountId, DelegationId, SystemIdGenerator, TestClock, UserId};
use postit_data::audit::AuditContext;
use postit_data::delegations::Delegation;
use postit_data::testkit::callers;
use postit_data::{Access, AccountLimit, Level, OwnerScope, ScopeError};
use postit_identity::delegation::{
    AccountsInput, ChangeRequest, DelegationError, DelegationService, GrantRequest, Workspace,
    expire_delegations, resolve_act_as,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    // Postgres keeps microseconds; round so values read back compare equal.
    Utc::now().trunc_subsecs(6)
}

fn service(pool: &PgPool, at: DateTime<Utc>) -> DelegationService {
    DelegationService::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(TestClock::new(at)),
    )
}

fn ctx() -> AuditContext {
    AuditContext::default()
}

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

async fn user(pool: &PgPool, name: &str, status: &str, email: Option<&str>) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                            role, status)
         VALUES ($1, 'https://i.test', $2, $3, $4, $5, 'member', $6)",
    )
    .bind(id)
    .bind(id.to_string())
    .bind(email)
    .bind(email.is_some())
    .bind(name)
    .bind(status)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("user: {e}"));
    UserId::from(id)
}

async fn active(pool: &PgPool, name: &str) -> UserId {
    user(pool, name, "active", None).await
}

async fn set_status(pool: &PgPool, id: UserId, status: &str) {
    sqlx::query("UPDATE users SET status = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(status)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("status: {e}"));
}

async fn account(pool: &PgPool, owner: UserId) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("platform: {e}"));
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, $3)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(id.to_string())
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("account: {e}"));
    AccountId::from(id)
}

fn request(delegate: UserId, level: Level) -> GrantRequest {
    GrantRequest {
        delegate_id: delegate,
        level,
        accounts: AccountsInput::All,
        expires_at: None,
    }
}

async fn grant(
    svc: &DelegationService,
    owner: UserId,
    req: GrantRequest,
) -> Result<Delegation, DelegationError> {
    svc.grant(&OwnerScope::own(owner), req, ctx())
        .await
        .map(|detail| detail.delegation)
}

async fn granted(svc: &DelegationService, owner: UserId, req: GrantRequest) -> Delegation {
    grant(svc, owner, req)
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"))
}

#[derive(sqlx::FromRow)]
struct Event {
    kind: String,
    actor: Option<Uuid>,
    owner: Option<Uuid>,
    subject: Option<Uuid>,
    details: Value,
}

async fn events(pool: &PgPool, kind: &str) -> Vec<Event> {
    sqlx::query_as(
        "SELECT kind, actor_user_id AS actor, owner_id AS owner, subject_user_id AS subject,
                details
         FROM audit_events WHERE kind LIKE $1 ORDER BY id",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("events: {e}"))
}

async fn delegation_events(pool: &PgPool) -> Vec<Event> {
    events(pool, "delegation_%").await
}

async fn row(
    pool: &PgPool,
    id: DelegationId,
) -> (Option<DateTime<Utc>>, Option<String>, Option<Uuid>) {
    sqlx::query_as("SELECT ended_at, ended_reason, revoked_by FROM delegations WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("row: {e}"))
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn ids(accounts: &[AccountId]) -> Value {
    Value::Array(
        accounts
            .iter()
            .map(|a| Value::String(a.as_uuid().to_string()))
            .collect(),
    )
}

fn invalid(result: Result<impl std::fmt::Debug, DelegationError>, expected: &str) {
    match result {
        Err(DelegationError::Invalid { field }) => assert_eq!(field, expected),
        other => unreachable!("expected Invalid({expected}), got {other:?}"),
    }
}

#[allow(clippy::needless_pass_by_value)] // Takes the call's result directly, like `invalid`.
fn not_found(result: Result<impl std::fmt::Debug, DelegationError>) {
    assert!(
        matches!(result, Err(DelegationError::NotFound)),
        "expected NotFound, got {result:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_records_audit_event(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let a = account(&pool, owner).await;
    let expires = now + Duration::days(1);
    let svc = service(&pool, now);

    let d = granted(
        &svc,
        owner,
        GrantRequest {
            delegate_id: delegate,
            level: Level::Edit,
            accounts: AccountsInput::Only(vec![a]),
            expires_at: Some(expires),
        },
    )
    .await;
    assert_eq!(d.owner_id, owner);
    assert_eq!(d.delegate_id, delegate);
    assert_eq!(d.level, Level::Edit);
    assert_eq!(d.accounts, AccountLimit::only([a]));
    assert_eq!(d.expires_at, Some(expires));
    assert_eq!(d.ended_at, None);
    assert_eq!(d.created_by, Some(owner));

    let events = delegation_events(&pool).await;
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e.kind, "delegation_granted");
    assert_eq!(e.actor, Some(owner.as_uuid()));
    assert_eq!(e.owner, Some(owner.as_uuid()));
    assert_eq!(e.subject, Some(delegate.as_uuid()));
    assert_eq!(
        e.details,
        json!({
            "delegation_id": d.id.as_uuid().to_string(),
            "level": "edit",
            "all_accounts": false,
            "account_ids": ids(&[a]),
            "expires_at": stamp(expires),
        })
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_refuses_self_inactive_and_unknown_delegate(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let svc = service(&pool, now);

    invalid(
        grant(&svc, owner, request(owner, Level::View)).await,
        "delegate_id",
    );
    for status in ["pending", "disabled", "deleting"] {
        let other = user(&pool, status, status, None).await;
        invalid(
            grant(&svc, owner, request(other, Level::View)).await,
            "delegate_id",
        );
    }
    let unknown = UserId::from(Uuid::now_v7());
    invalid(
        grant(&svc, owner, request(unknown, Level::View)).await,
        "delegate_id",
    );

    // Another owner's account is not a valid limit.
    let delegate = active(&pool, "Delegate").await;
    let stranger = active(&pool, "Stranger").await;
    let foreign = account(&pool, stranger).await;
    let mut req = request(delegate, Level::View);
    req.accounts = AccountsInput::Only(vec![foreign]);
    invalid(grant(&svc, owner, req).await, "account_ids");

    assert!(delegation_events(&pool).await.is_empty());
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM delegations")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(rows, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_refuses_past_expiry(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let svc = service(&pool, now);
    for expires in [now, now - Duration::seconds(1)] {
        let mut req = request(delegate, Level::View);
        req.expires_at = Some(expires);
        invalid(grant(&svc, owner, req).await, "expires_at");
    }
    let mut req = request(delegate, Level::View);
    req.expires_at = Some(now + Duration::seconds(1));
    granted(&svc, owner, req).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_twice_is_exists(pool: PgPool) {
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let svc = service(&pool, t0());
    granted(&svc, owner, request(delegate, Level::View)).await;
    let again = grant(&svc, owner, request(delegate, Level::Publish)).await;
    assert!(
        matches!(again, Err(DelegationError::Exists)),
        "got {again:?}"
    );
    assert_eq!(delegation_events(&pool).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn regrant_over_expired_unended_grant_ends_it_first(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let mut req = request(delegate, Level::View);
    req.expires_at = Some(now + Duration::hours(1));
    let old = granted(&service(&pool, now), owner, req).await;

    let later = now + Duration::hours(2);
    let new = granted(
        &service(&pool, later),
        owner,
        request(delegate, Level::Edit),
    )
    .await;
    assert_ne!(new.id, old.id);
    assert_eq!(new.level, Level::Edit);
    assert_eq!(
        row(&pool, old.id).await,
        (Some(later), Some("expired".to_string()), None)
    );

    let events = delegation_events(&pool).await;
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "delegation_granted",
            "delegation_expired",
            "delegation_granted"
        ]
    );
    let expired = &events[1];
    assert_eq!(expired.actor, None);
    assert_eq!(expired.owner, Some(owner.as_uuid()));
    assert_eq!(expired.subject, Some(delegate.as_uuid()));
    assert_eq!(
        expired.details["delegation_id"],
        json!(old.id.as_uuid().to_string())
    );
    assert_eq!(
        events[2].details["delegation_id"],
        json!(new.id.as_uuid().to_string())
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegated_scope_cannot_grant_change_revoke_or_list(pool: PgPool) {
    let owner = active(&pool, "Owner").await;
    let target = active(&pool, "Target").await;
    let c = callers(&pool, owner, &[]).await;
    let svc = service(&pool, t0());
    let existing = granted(&svc, owner, request(target, Level::View)).await;

    for scope in [&c.view, &c.edit, &c.publish, &c.limited] {
        assert!(matches!(scope.access, Access::Delegated { .. }));
        let r = svc
            .grant(scope, request(c.stranger.actor, Level::View), ctx())
            .await;
        assert!(
            matches!(r, Err(DelegationError::Scope(ScopeError::OwnerOnly))),
            "grant: {r:?}"
        );
        let r = svc
            .change(
                scope,
                existing.id,
                ChangeRequest {
                    level: Some(Level::Publish),
                    accounts: None,
                    expires_at: None,
                },
                ctx(),
            )
            .await;
        assert!(
            matches!(r, Err(DelegationError::Scope(ScopeError::OwnerOnly))),
            "change: {r:?}"
        );
        let r = svc.revoke(scope, existing.id, ctx()).await;
        assert!(
            matches!(r, Err(DelegationError::Scope(ScopeError::OwnerOnly))),
            "revoke: {r:?}"
        );
        let r = svc.list_granted(scope, true, page()).await;
        assert!(
            matches!(r, Err(DelegationError::Scope(ScopeError::OwnerOnly))),
            "list: {r:?}"
        );
    }
    let still = svc
        .list_granted(&OwnerScope::own(owner), false, page())
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    let target_row = still
        .data
        .iter()
        .find(|v| v.delegation.id == existing.id)
        .unwrap_or_else(|| unreachable!("grant missing"));
    assert_eq!(target_row.delegation.level, Level::View);
    assert_eq!(target_row.delegation.ended_at, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_level_and_limit_audits_before_and_after(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let a = account(&pool, owner).await;
    let b = account(&pool, owner).await;
    let svc = service(&pool, now);
    let scope = OwnerScope::own(owner);
    let d = granted(&svc, owner, request(delegate, Level::Edit)).await;
    let expires = now + Duration::days(2);

    let changed = svc
        .change(
            &scope,
            d.id,
            ChangeRequest {
                level: Some(Level::Publish),
                accounts: Some(AccountsInput::Only(vec![b, a])),
                expires_at: Some(Some(expires)),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("change: {e}"))
        .delegation;
    assert_eq!(changed.level, Level::Publish);
    assert_eq!(changed.accounts, AccountLimit::only([a, b]));
    assert_eq!(changed.expires_at, Some(expires));

    let events = events(&pool, "delegation_changed").await;
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e.actor, Some(owner.as_uuid()));
    assert_eq!(e.owner, Some(owner.as_uuid()));
    assert_eq!(e.subject, Some(delegate.as_uuid()));
    let mut sorted = [a, b];
    sorted.sort_by_key(AccountId::as_uuid);
    assert_eq!(
        e.details,
        json!({
            "delegation_id": d.id.as_uuid().to_string(),
            "before": {
                "level": "edit",
                "all_accounts": true,
                "account_ids": [],
                "expires_at": null,
            },
            "after": {
                "level": "publish",
                "all_accounts": false,
                "account_ids": ids(&sorted),
                "expires_at": stamp(expires),
            },
        })
    );

    // Back to every account, with the expiry cleared.
    let back = svc
        .change(
            &scope,
            d.id,
            ChangeRequest {
                level: None,
                accounts: Some(AccountsInput::All),
                expires_at: Some(None),
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("change back: {e}"))
        .delegation;
    assert_eq!(back.level, Level::Publish);
    assert_eq!(back.accounts, AccountLimit::All);
    assert_eq!(back.expires_at, None);

    // Invalid input changes nothing and records nothing.
    let stranger = active(&pool, "Stranger").await;
    let foreign = account(&pool, stranger).await;
    invalid(
        svc.change(
            &scope,
            d.id,
            ChangeRequest {
                level: None,
                accounts: Some(AccountsInput::Only(vec![foreign])),
                expires_at: None,
            },
            ctx(),
        )
        .await,
        "account_ids",
    );
    invalid(
        svc.change(
            &scope,
            d.id,
            ChangeRequest {
                level: None,
                accounts: None,
                expires_at: Some(Some(now)),
            },
            ctx(),
        )
        .await,
        "expires_at",
    );
    assert_eq!(count_events(&pool, "delegation_changed").await, 2);
}

async fn count_events(pool: &PgPool, kind: &str) -> usize {
    events(pool, kind).await.len()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_on_ended_or_expired_is_not_found(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let level_only = || ChangeRequest {
        level: Some(Level::Publish),
        accounts: None,
        expires_at: None,
    };

    let revoked = granted(&svc, owner, request(active(&pool, "R").await, Level::View)).await;
    svc.revoke(&scope, revoked.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    not_found(svc.change(&scope, revoked.id, level_only(), ctx()).await);

    let mut req = request(active(&pool, "E").await, Level::View);
    req.expires_at = Some(now + Duration::hours(1));
    let expiring = granted(&svc, owner, req).await;
    let later = service(&pool, now + Duration::hours(1));
    not_found(later.change(&scope, expiring.id, level_only(), ctx()).await);

    not_found(
        svc.change(
            &scope,
            DelegationId::from(Uuid::now_v7()),
            level_only(),
            ctx(),
        )
        .await,
    );

    let other_owner = active(&pool, "Other").await;
    let others = granted(
        &svc,
        other_owner,
        request(active(&pool, "O").await, Level::View),
    )
    .await;
    not_found(svc.change(&scope, others.id, level_only(), ctx()).await);

    assert!(events(&pool, "delegation_changed").await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_is_idempotent_with_one_event(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let d = granted(&svc, owner, request(delegate, Level::Edit)).await;

    svc.revoke(&scope, d.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    service(&pool, now + Duration::minutes(5))
        .revoke(&scope, d.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke again: {e}"));
    assert_eq!(
        row(&pool, d.id).await,
        (
            Some(now),
            Some("revoked".to_string()),
            Some(owner.as_uuid())
        )
    );

    let events = events(&pool, "delegation_revoked").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor, Some(owner.as_uuid()));
    assert_eq!(events[0].owner, Some(owner.as_uuid()));
    assert_eq!(events[0].subject, Some(delegate.as_uuid()));
    assert_eq!(
        events[0].details["delegation_id"],
        json!(d.id.as_uuid().to_string())
    );

    // Another owner's or an unknown grant is not found.
    let other = active(&pool, "Other").await;
    not_found(svc.revoke(&OwnerScope::own(other), d.id, ctx()).await);
    not_found(
        svc.revoke(&scope, DelegationId::from(Uuid::now_v7()), ctx())
            .await,
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn leave_by_delegate_and_not_by_stranger(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let stranger = active(&pool, "Stranger").await;
    let svc = service(&pool, now);
    let d = granted(&svc, owner, request(delegate, Level::Edit)).await;

    not_found(svc.leave(stranger, d.id, ctx()).await);
    not_found(svc.leave(owner, d.id, ctx()).await);
    assert_eq!(row(&pool, d.id).await, (None, None, None));

    svc.leave(delegate, d.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("leave: {e}"));
    svc.leave(delegate, d.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("leave again: {e}"));
    assert_eq!(
        row(&pool, d.id).await,
        (Some(now), Some("left".to_string()), None)
    );
    let events = events(&pool, "delegation_left").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor, Some(delegate.as_uuid()));
    assert_eq!(events[0].owner, Some(owner.as_uuid()));
    assert_eq!(events[0].subject, Some(delegate.as_uuid()));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoke_expired_unended_records_expired(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let delegate = active(&pool, "Delegate").await;
    let leaver = active(&pool, "Leaver").await;
    let mut req = request(delegate, Level::View);
    req.expires_at = Some(now + Duration::hours(1));
    let revoked = granted(&svc, owner, req).await;
    let mut req = request(leaver, Level::View);
    req.expires_at = Some(now + Duration::hours(1));
    let left = granted(&svc, owner, req).await;

    let later_at = now + Duration::hours(2);
    let later = service(&pool, later_at);
    later
        .revoke(&scope, revoked.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    later
        .leave(leaver, left.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("leave: {e}"));

    for d in [&revoked, &left] {
        assert_eq!(
            row(&pool, d.id).await,
            (Some(later_at), Some("expired".to_string()), None)
        );
    }
    let expired = events(&pool, "delegation_expired").await;
    assert_eq!(expired.len(), 2);
    assert!(expired.iter().all(|e| e.actor.is_none()));
    assert!(events(&pool, "delegation_revoked").await.is_empty());
    assert!(events(&pool, "delegation_left").await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expire_delegations_ends_due_grants_and_audits(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let svc = service(&pool, now);
    let mut due = Vec::new();
    for (minutes, name) in [(10, "A"), (11, "B"), (12, "C")] {
        let delegate = active(&pool, name).await;
        let mut req = request(delegate, Level::View);
        req.expires_at = Some(now + Duration::minutes(minutes));
        due.push(granted(&svc, owner, req).await);
    }
    let later = active(&pool, "Later").await;
    let mut req = request(later, Level::Edit);
    req.expires_at = Some(now + Duration::days(1));
    let future = granted(&svc, owner, req).await;

    let at = now + Duration::hours(1);
    let count = expire_delegations(&pool, &SystemIdGenerator, at)
        .await
        .unwrap_or_else(|e| unreachable!("expire: {e}"));
    assert_eq!(count, 3);
    for d in &due {
        assert_eq!(
            row(&pool, d.id).await,
            (Some(at), Some("expired".to_string()), None)
        );
    }
    assert_eq!(row(&pool, future.id).await, (None, None, None));
    let expired = events(&pool, "delegation_expired").await;
    assert_eq!(expired.len(), 3);
    assert!(expired.iter().all(|e| e.actor.is_none()));
    assert!(
        expired
            .iter()
            .all(|e| e.owner == Some(owner.as_uuid()) && e.subject.is_some())
    );
    for (d, (minutes, _)) in due.iter().zip([(10, "A"), (11, "B"), (12, "C")]) {
        let event = expired
            .iter()
            .find(|e| e.details["delegation_id"] == d.id.as_uuid().to_string())
            .unwrap_or_else(|| unreachable!("no expired event for {:?}", d.id));
        assert_eq!(event.subject, Some(d.delegate_id.as_uuid()));
        assert_eq!(
            event.details,
            json!({
                "delegation_id": d.id.as_uuid().to_string(),
                "level": "view",
                "all_accounts": true,
                "account_ids": [],
                "expires_at": stamp(now + Duration::minutes(minutes)),
            })
        );
    }

    let again = expire_delegations(&pool, &SystemIdGenerator, at)
        .await
        .unwrap_or_else(|e| unreachable!("expire again: {e}"));
    assert_eq!(again, 0);
    assert_eq!(events(&pool, "delegation_expired").await.len(), 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn workspaces_lists_own_then_received_hiding_inactive_owners(pool: PgPool) {
    let now = t0();
    let me = active(&pool, "Me").await;
    let svc = service(&pool, now);

    let a = active(&pool, "Alpha").await;
    let ga = granted(&svc, a, request(me, Level::Edit)).await;
    let b = active(&pool, "Bravo").await;
    let b1 = account(&pool, b).await;
    let mut req = request(me, Level::View);
    req.accounts = AccountsInput::Only(vec![b1]);
    let gb = granted(&svc, b, req).await;

    let disabled = active(&pool, "Disabled").await;
    granted(&svc, disabled, request(me, Level::Edit)).await;
    set_status(&pool, disabled, "disabled").await;
    let deleting = active(&pool, "Deleting").await;
    granted(&svc, deleting, request(me, Level::Edit)).await;
    set_status(&pool, deleting, "deleting").await;
    let expiring = active(&pool, "Expiring").await;
    let mut req = request(me, Level::Edit);
    req.expires_at = Some(now + Duration::minutes(30));
    granted(&svc, expiring, req).await;
    let revoking = active(&pool, "Revoking").await;
    let gr = granted(&svc, revoking, request(me, Level::Edit)).await;
    svc.revoke(&OwnerScope::own(revoking), gr.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    // A grant `me` gave is not a workspace `me` can enter.
    granted(&svc, me, request(a, Level::View)).await;

    let list = service(&pool, now + Duration::hours(1))
        .workspaces(me)
        .await
        .unwrap_or_else(|e| unreachable!("workspaces: {e}"));
    assert_eq!(
        list[0],
        Workspace {
            owner_id: me,
            owner_display_name: "Me".to_string(),
            access: None,
        }
    );
    let mut rest = list[1..].to_vec();
    rest.sort_by(|x, y| x.owner_display_name.cmp(&y.owner_display_name));
    assert_eq!(
        rest,
        vec![
            Workspace {
                owner_id: a,
                owner_display_name: "Alpha".to_string(),
                access: Some((ga.id, Level::Edit, AccountLimit::All)),
            },
            Workspace {
                owner_id: b,
                owner_display_name: "Bravo".to_string(),
                access: Some((gb.id, Level::View, AccountLimit::only([b1]))),
            },
        ]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lookup_exact_self_and_duplicate(pool: PgPool) {
    let caller = user(&pool, "Caller", "active", Some("me@example.com")).await;
    let target = user(&pool, "Target", "active", Some("target@example.com")).await;
    user(&pool, "Dup One", "active", Some("dup@example.com")).await;
    user(&pool, "Dup Two", "active", Some("dup@example.com")).await;
    let svc = service(&pool, t0());

    let found = svc
        .lookup(caller, "  Target@Example.com ")
        .await
        .unwrap_or_else(|e| unreachable!("lookup: {e}"));
    assert_eq!(found, Some((target, "Target".to_string())));

    for email in [
        "me@example.com",
        "dup@example.com",
        "target@example",
        "nobody@example.com",
    ] {
        let found = svc
            .lookup(caller, email)
            .await
            .unwrap_or_else(|e| unreachable!("lookup {email}: {e}"));
        assert_eq!(found, None, "{email}");
    }
    for blank in ["", "   "] {
        invalid(svc.lookup(caller, blank).await, "email");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn resolve_after_revoke_level_change_and_disable(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let d = granted(&svc, owner, request(delegate, Level::Edit)).await;

    let level = |s: Option<OwnerScope>| match s.map(|s| s.access) {
        Some(Access::Delegated { level, .. }) => Some(level),
        Some(Access::Owner) => unreachable!("own scope from resolution"),
        None => None,
    };
    let resolve = || async {
        resolve_act_as(&pool, delegate, owner, now)
            .await
            .unwrap_or_else(|e| unreachable!("resolve: {e}"))
    };

    assert_eq!(level(resolve().await), Some(Level::Edit));
    assert_eq!(
        resolve_act_as(&pool, owner, owner, now)
            .await
            .unwrap_or_else(|e| unreachable!("resolve self: {e}"))
            .map(|s| s.owner),
        None
    );

    svc.change(
        &scope,
        d.id,
        ChangeRequest {
            level: Some(Level::View),
            accounts: None,
            expires_at: None,
        },
        ctx(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("change: {e}"));
    assert_eq!(level(resolve().await), Some(Level::View));

    set_status(&pool, delegate, "disabled").await;
    assert_eq!(level(resolve().await), None);
    set_status(&pool, delegate, "active").await;
    assert_eq!(level(resolve().await), Some(Level::View));
    set_status(&pool, owner, "disabled").await;
    assert_eq!(level(resolve().await), None);
    set_status(&pool, owner, "active").await;

    svc.revoke(&scope, d.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    assert_eq!(level(resolve().await), None);
}

/// Inserts `count` active owners, each with one live grant to `delegate` (expiring at
/// `expires_at`, if given), straight through SQL.
async fn bulk_grants(
    pool: &PgPool,
    delegate: UserId,
    count: i32,
    expires_at: Option<DateTime<Utc>>,
) {
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
         SELECT gen_random_uuid(), 'https://i.test', 'bulk-' || n, 'Owner ' || n,
                'member', 'active'
         FROM generate_series(1, $1) n",
    )
    .bind(count)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("bulk users: {e}"));
    sqlx::query(
        "INSERT INTO delegations (id, owner_id, delegate_id, level, all_accounts, expires_at)
         SELECT gen_random_uuid(), u.id, $1, 'view', TRUE, $2
         FROM users u WHERE u.oidc_subject LIKE 'bulk-%'",
    )
    .bind(delegate.as_uuid())
    .bind(expires_at)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("bulk grants: {e}"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expire_delegations_handles_more_than_one_batch(pool: PgPool) {
    let now = t0();
    let delegate = active(&pool, "Delegate").await;
    bulk_grants(&pool, delegate, 450, Some(now + Duration::minutes(5))).await;

    let at = now + Duration::hours(1);
    let count = expire_delegations(&pool, &SystemIdGenerator, at)
        .await
        .unwrap_or_else(|e| unreachable!("expire: {e}"));
    assert_eq!(count, 450);
    let (live,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM delegations WHERE ended_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("live: {e}"));
    assert_eq!(live, 0);
    let (events, distinct): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(DISTINCT details ->> 'delegation_id')
         FROM audit_events WHERE kind = 'delegation_expired'",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("events: {e}"));
    assert_eq!((events, distinct), (450, 450), "one event per grant");
    let again = expire_delegations(&pool, &SystemIdGenerator, at)
        .await
        .unwrap_or_else(|e| unreachable!("again: {e}"));
    assert_eq!(again, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn workspaces_returns_every_received_grant_beyond_one_page(pool: PgPool) {
    let me = active(&pool, "Me").await;
    bulk_grants(&pool, me, 250, None).await;

    let list = service(&pool, t0())
        .workspaces(me)
        .await
        .unwrap_or_else(|e| unreachable!("workspaces: {e}"));
    assert_eq!(list.len(), 251, "own workspace plus every grant");
    let owners: std::collections::HashSet<_> = list.iter().map(|w| w.owner_id).collect();
    assert_eq!(owners.len(), 251, "none repeated");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_and_change_return_both_parties_from_the_transaction(pool: PgPool) {
    let owner = active(&pool, "Owner").await;
    let delegate = active(&pool, "Delegate").await;
    let svc = service(&pool, t0());

    let detail = svc
        .grant(
            &OwnerScope::own(owner),
            request(delegate, Level::View),
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"));
    assert_eq!(detail.owner.id, owner);
    assert_eq!(detail.owner.display_name, "Owner");
    assert_eq!(detail.delegate.id, delegate);
    assert_eq!(detail.delegate.display_name, "Delegate");

    let changed = svc
        .change(
            &OwnerScope::own(owner),
            detail.delegation.id,
            ChangeRequest {
                level: Some(Level::Edit),
                ..ChangeRequest::default()
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("change: {e}"));
    assert_eq!(changed.delegation.level, Level::Edit);
    assert_eq!(changed.delegate.display_name, "Delegate");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn empty_change_is_a_no_op_without_an_event(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let grant = granted(&svc, owner, request(active(&pool, "D").await, Level::Edit)).await;
    let events_before = delegation_events(&pool).await.len();

    let same = svc
        .change(&scope, grant.id, ChangeRequest::default(), ctx())
        .await
        .unwrap_or_else(|e| unreachable!("change: {e}"));
    assert_eq!(same.delegation, grant);
    assert_eq!(delegation_events(&pool).await.len(), events_before);
    assert!(events(&pool, "delegation_changed").await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_checks_liveness_before_the_expiry_value(pool: PgPool) {
    let now = t0();
    let owner = active(&pool, "Owner").await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool, now);
    let past = || ChangeRequest {
        level: None,
        accounts: None,
        expires_at: Some(Some(now - Duration::hours(1))),
    };

    let ended = granted(&svc, owner, request(active(&pool, "E").await, Level::View)).await;
    svc.revoke(&scope, ended.id, ctx())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    not_found(svc.change(&scope, ended.id, past(), ctx()).await);
    not_found(
        svc.change(&scope, DelegationId::from(Uuid::now_v7()), past(), ctx())
            .await,
    );

    let live = granted(&svc, owner, request(active(&pool, "L").await, Level::View)).await;
    invalid(
        svc.change(&scope, live.id, past(), ctx()).await,
        "expires_at",
    );
}
