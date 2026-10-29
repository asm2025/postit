//! Delegation and pending-user notifications recorded by the identity services.

use std::sync::Arc;

use chrono::{DateTime, Duration, SubsecRound, Utc};
use postit_config::{BootstrapSettings, UserinfoMode};
use postit_core::{
    DelegationEvent, DelegationId, Notice, SystemIdGenerator, TestClock, UserId, UserPendingNotice,
};
use postit_data::audit::AuditContext;
use postit_data::notifications::NotificationRow;
use postit_data::{Level, OwnerScope};
use postit_i18n::Locale;
use postit_identity::delegation::{
    AccountsInput, ChangeRequest, DelegationService, GrantRequest, expire_delegations,
};
use postit_identity::testkit::{TestIssuer, claims_transformer};
use postit_notify::{Notifier, notifications_for};
use sqlx::PgPool;
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

fn service(pool: &PgPool, at: DateTime<Utc>) -> DelegationService {
    DelegationService::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(TestClock::new(at)),
        Notifier::for_tests(pool.clone()),
    )
}

async fn user(pool: &PgPool, name: &str, role: &str, status: &str) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                            role, status)
         VALUES ($1, 'https://i.test', $2, $3, true, $4, $5, $6)",
    )
    .bind(id)
    .bind(id.to_string())
    .bind(format!("{}@private.example", name.to_lowercase()))
    .bind(name)
    .bind(role)
    .bind(status)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("user: {e}"));
    UserId::from(id)
}

async fn member(pool: &PgPool, name: &str) -> UserId {
    user(pool, name, "member", "active").await
}

async fn rows(pool: &PgPool, user: UserId) -> Vec<NotificationRow> {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    notifications_for(&mut conn, user).await
}

/// The delegation events and party names a user was told about, oldest first.
async fn told(pool: &PgPool, user: UserId) -> Vec<(DelegationEvent, String)> {
    let mut found: Vec<_> = rows(pool, user)
        .await
        .into_iter()
        .filter_map(|row| match row.notice {
            Notice::DelegationChanged(n) => Some((row.created_at, n.event, n.party_name)),
            _ => None,
        })
        .collect();
    found.sort_by_key(|(at, ..)| *at);
    found.into_iter().map(|(_, e, n)| (e, n)).collect()
}

fn request(delegate: UserId) -> GrantRequest {
    GrantRequest {
        delegate_id: delegate,
        level: Level::Edit,
        accounts: AccountsInput::All,
        expires_at: None,
    }
}

async fn grant(svc: &DelegationService, owner: UserId, delegate: UserId) -> DelegationId {
    svc.grant(
        &OwnerScope::own(owner),
        request(delegate),
        AuditContext::default(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"))
    .delegation
    .id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_change_revoke_notify_the_delegate(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let svc = service(&pool, t0());
    let scope = OwnerScope::own(owner);

    let id = grant(&svc, owner, delegate).await;
    svc.change(
        &scope,
        id,
        ChangeRequest {
            level: Some(Level::View),
            ..ChangeRequest::default()
        },
        AuditContext::default(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("change: {e}"));
    svc.revoke(&scope, id, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));

    assert_eq!(
        told(&pool, delegate).await,
        vec![
            (DelegationEvent::Granted, "Olivia".to_owned()),
            (DelegationEvent::Changed, "Olivia".to_owned()),
            (DelegationEvent::Revoked, "Olivia".to_owned()),
        ]
    );
    assert!(
        rows(&pool, owner).await.is_empty(),
        "the owner caused all three"
    );
    let first = rows(&pool, delegate).await;
    assert!(first.iter().all(|r| r.owner == Some(owner)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_change_that_changes_nothing_notifies_no_one(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let svc = service(&pool, t0());
    let id = grant(&svc, owner, delegate).await;

    svc.change(
        &OwnerScope::own(owner),
        id,
        ChangeRequest::default(),
        AuditContext::default(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("change: {e}"));

    assert_eq!(told(&pool, delegate).await.len(), 1, "only the grant");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn leave_notifies_the_owner(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let svc = service(&pool, t0());
    let id = grant(&svc, owner, delegate).await;

    svc.leave(delegate, id, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("leave: {e}"));

    assert_eq!(
        told(&pool, owner).await,
        vec![(DelegationEvent::Left, "Dan".to_owned())]
    );
    assert_eq!(told(&pool, delegate).await.len(), 1, "only the grant");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn leaving_an_ended_grant_notifies_no_one_twice(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let svc = service(&pool, t0());
    let id = grant(&svc, owner, delegate).await;
    for _ in 0..2 {
        svc.leave(delegate, id, AuditContext::default())
            .await
            .unwrap_or_else(|e| unreachable!("leave: {e}"));
    }

    assert_eq!(told(&pool, owner).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expiry_notifies_both(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let now = t0();
    let svc = service(&pool, now);
    svc.grant(
        &OwnerScope::own(owner),
        GrantRequest {
            expires_at: Some(now + Duration::hours(1)),
            ..request(delegate)
        },
        AuditContext::default(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"));

    let ended = expire_delegations(
        &pool,
        &SystemIdGenerator,
        &Notifier::for_tests(pool.clone()),
        now + Duration::hours(2),
    )
    .await
    .unwrap_or_else(|e| unreachable!("expire: {e}"));

    assert_eq!(ended, 1);
    assert_eq!(
        told(&pool, owner).await,
        vec![(DelegationEvent::Expired, "Dan".to_owned())]
    );
    assert_eq!(
        told(&pool, delegate).await,
        vec![
            (DelegationEvent::Granted, "Olivia".to_owned()),
            (DelegationEvent::Expired, "Olivia".to_owned()),
        ]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revoking_an_expired_grant_tells_both_it_expired(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let now = t0();
    let granting = service(&pool, now);
    let id = granting
        .grant(
            &OwnerScope::own(owner),
            GrantRequest {
                expires_at: Some(now + Duration::hours(1)),
                ..request(delegate)
            },
            AuditContext::default(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"))
        .delegation
        .id;

    service(&pool, now + Duration::hours(2))
        .revoke(&OwnerScope::own(owner), id, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));

    assert_eq!(
        told(&pool, owner).await,
        vec![(DelegationEvent::Expired, "Dan".to_owned())]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_sign_in_notifies_every_active_admin_once(pool: PgPool) {
    let first = user(&pool, "Ann", "admin", "active").await;
    let second = user(&pool, "Ben", "admin", "active").await;
    let disabled = user(&pool, "Cy", "admin", "disabled").await;
    let issuer = TestIssuer::start().await;
    let transformer = claims_transformer(
        pool.clone(),
        &issuer,
        UserinfoMode::Never,
        BootstrapSettings {
            admin_email: None,
            admin_subject: None,
        },
    );
    let (bearer, claims) = issuer.token_and_claims(
        "newcomer",
        Some("new@private.example"),
        Some(true),
        Some("Newcomer"),
    );

    let newcomer = transformer
        .transform(&claims, &bearer, AuditContext::default(), Locale::EnUs)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));
    // A later sign-in of the same still-pending user adds nothing.
    transformer
        .transform(&claims, &bearer, AuditContext::default(), Locale::EnUs)
        .await
        .unwrap_or_else(|e| unreachable!("second transform: {e}"));

    for admin in [first, second] {
        let rows = rows(&pool, admin).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].notice,
            Notice::UserPending(UserPendingNotice {
                user_id: newcomer.id
            })
        );
        assert_eq!(rows[0].owner, None);
    }
    assert!(rows(&pool, disabled).await.is_empty());
    assert!(rows(&pool, newcomer.id).await.is_empty());
    let emails: i64 =
        sqlx::query_scalar("SELECT count(*) FROM job_outbox WHERE job_type = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(
        emails, 2,
        "the approval emails are unchanged: one per admin"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_bootstrap_admin_sign_in_notifies_no_one(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let transformer = claims_transformer(
        pool.clone(),
        &issuer,
        UserinfoMode::Never,
        BootstrapSettings {
            admin_email: Some("root@private.example".into()),
            admin_subject: None,
        },
    );
    let (bearer, claims) = issuer.token_and_claims(
        "root",
        Some("root@private.example"),
        Some(true),
        Some("Root"),
    );

    let root = transformer
        .transform(&claims, &bearer, AuditContext::default(), Locale::EnUs)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert!(rows(&pool, root.id).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn notification_payload_names_only_the_party(pool: PgPool) {
    let owner = member(&pool, "Olivia").await;
    let delegate = member(&pool, "Dan").await;
    let svc = service(&pool, t0());
    let id = grant(&svc, owner, delegate).await;
    svc.leave(delegate, id, AuditContext::default())
        .await
        .unwrap_or_else(|e| unreachable!("leave: {e}"));

    for user in [owner, delegate] {
        for row in rows(&pool, user).await {
            let json = serde_json::to_string(&row.notice)
                .unwrap_or_else(|e| unreachable!("serialize: {e}"));
            assert!(!json.contains('@'), "no email address in {json}");
            assert!(!json.contains("private.example"), "no address in {json}");
        }
    }
    let stored: Vec<String> = sqlx::query_scalar("SELECT payload::text FROM notifications")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("payloads: {e}"));
    assert!(!stored.is_empty());
    assert!(stored.iter().all(|p| !p.contains('@')));
}
