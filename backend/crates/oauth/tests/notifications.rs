//! Owner notifications for connection events: a grant that needs a reconnect, a platform that
//! revoked it, and the `reconnect_by` reminders and deadline.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use common::{
    CLIENT_SECRET, oauth_agent, oauth_config, oauth_grant, oauth_provider, platform, platforms,
    vault,
};
use hmac::{Hmac, Mac};
use postit_agent_kit::testkit::{MockOAuthProvider, OAuthFailure};
use postit_agent_mock::oauth::{DEAUTHORIZE, SIGNATURE_HEADER};
use postit_core::{
    CredentialStore, Credentials, GrantId, InboundEvent, Notice, NotificationKind, ReminderStage,
    UserId,
};
use postit_data::accounts::GrantRepo;
use postit_data::audit::AuditContext;
use postit_data::notifications::NotificationRow;
use postit_data::testkit::{grant, seed_user};
use postit_data::{AccountLimit, Level, OwnerScope};
use postit_notify::notifications_for;
use postit_oauth::jobs::run_refresh_credentials;
use postit_oauth::testkit::{TestOptions, account_service_with, test_accounts_settings};
use postit_oauth::{AccountService, PgCredentialStore};
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;

const OAUTH: &str = "mock_oauth";

struct Fixture {
    pool: PgPool,
    service: AccountService,
    provider: MockOAuthProvider,
    owner: UserId,
}

async fn fixture(pool: &PgPool) -> Fixture {
    platforms(pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let (agent, _) = oauth_agent(&provider, OAUTH);
    let mut configs = BTreeMap::new();
    configs.insert(platform(OAUTH), Some(oauth_config()));
    let service = account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        TestOptions {
            configs,
            ..TestOptions::default()
        },
    );
    let owner = seed_user(pool, "active").await;
    Fixture {
        pool: pool.clone(),
        service,
        provider,
        owner,
    }
}

async fn rows(f: &Fixture, user: UserId) -> Vec<NotificationRow> {
    let mut conn = f.pool.acquire().await.unwrap();
    notifications_for(&mut conn, user).await
}

fn kinds(rows: &[NotificationRow]) -> Vec<NotificationKind> {
    rows.iter().map(|r| r.notice.kind()).collect()
}

fn names(rows: &[NotificationRow]) -> Vec<String> {
    let mut names: Vec<String> = rows
        .iter()
        .map(|r| match &r.notice {
            Notice::AccountNeedsReconnect(n) | Notice::AccountRevokedByPlatform(n) => {
                assert_eq!(n.account.platform, platform(OAUTH));
                n.account.account_name.clone()
            }
            Notice::ReconnectByPassed(n) | Notice::ReconnectReminder(n) => {
                assert_eq!(n.account.platform, platform(OAUTH));
                n.account.account_name.clone()
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    names.sort();
    names
}

async fn set_reconnect_by(f: &Fixture, grant: GrantId, offset: Duration) {
    sqlx::query("UPDATE account_grants SET reconnect_by = $2 WHERE id = $1")
        .bind(grant.as_uuid())
        .bind(Utc::now() + offset)
        .execute(&f.pool)
        .await
        .unwrap();
}

async fn warned(f: &Fixture, grant: GrantId) -> String {
    sqlx::query_scalar("SELECT reconnect_warned FROM account_grants WHERE id = $1")
        .bind(grant.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn run(f: &Fixture) {
    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
}

fn stage_of(row: &NotificationRow) -> Option<ReminderStage> {
    match &row.notice {
        Notice::ReconnectReminder(n) | Notice::ReconnectByPassed(n) => n.stage,
        other => panic!("unexpected {other:?}"),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn needs_reconnect_notifies_owner_per_account(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1", "acct-2"]).await;

    f.service
        .mark_needs_reconnect(&OwnerScope::own(f.owner), g, AuditContext::default())
        .await
        .unwrap();

    let got = rows(&f, f.owner).await;
    assert_eq!(
        kinds(&got),
        [
            NotificationKind::AccountNeedsReconnect,
            NotificationKind::AccountNeedsReconnect
        ]
    );
    assert_eq!(names(&got), ["acct-1", "acct-2"]);
    assert!(got.iter().all(|r| r.owner == Some(f.owner)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_grant_on_refresh_notifies(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1"]).await;
    let store = PgCredentialStore::new(f.service.clone(), f.owner, g);
    let Credentials::OAuth(stale) = store.credentials().await.unwrap() else {
        panic!("expected tokens");
    };
    f.provider.fail_next(OAuthFailure::InvalidGrant);

    assert!(store.refresh(&stale).await.is_err());

    let got = rows(&f, f.owner).await;
    assert_eq!(kinds(&got), [NotificationKind::AccountNeedsReconnect]);
    assert_eq!(names(&got), ["acct-1"]);
}

/// Replaces the grant's credential with an access token that has expired and no refresh token.
async fn expire_without_refresh_token(f: &Fixture, g: GrantId) {
    let sealed = vault()
        .seal(
            &json!({"access_token": "old", "refresh_token": null}),
            &GrantRepo::seal_context(g),
        )
        .unwrap();
    let mut conn = f.pool.acquire().await.unwrap();
    GrantRepo::store_tokens(
        &mut conn,
        g,
        &sealed,
        Some(Utc::now() - Duration::minutes(30)),
    )
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_token_without_refresh_token_notifies(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1"]).await;
    expire_without_refresh_token(&f, g).await;

    assert!(f.service.refresh_grant(g, None).await.is_err());

    let got = rows(&f, f.owner).await;
    assert_eq!(kinds(&got), [NotificationKind::AccountNeedsReconnect]);
    assert_eq!(names(&got), ["acct-1"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reminder_names_each_account_on_the_grant(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1", "acct-2"]).await;
    set_reconnect_by(&f, g, Duration::days(5)).await;

    run(&f).await;

    let got = rows(&f, f.owner).await;
    assert_eq!(
        kinds(&got),
        [
            NotificationKind::ReconnectReminder,
            NotificationKind::ReconnectReminder
        ]
    );
    assert_eq!(names(&got), ["acct-1", "acct-2"]);
    for row in &got {
        let Notice::ReconnectReminder(n) = &row.notice else {
            panic!()
        };
        assert_eq!(n.grant_id, g);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deauthorize_notifies_revoked(pool: PgPool) {
    let f = fixture(&pool).await;
    oauth_grant(
        &pool,
        f.owner,
        OAUTH,
        Some("mock-user"),
        "r",
        &["acct-1", "acct-2"],
    )
    .await;
    let other = seed_user(&pool, "active").await;
    oauth_grant(&pool, other, OAUTH, Some("someone-else"), "r2", &["acct-3"]).await;

    let body = serde_json::to_vec(&json!({ "user_id": "mock-user" })).unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(CLIENT_SECRET.as_bytes()).unwrap();
    mac.update(&body);
    let signature = hex::encode(mac.finalize().into_bytes());
    f.service
        .handle_platform_event(
            OAUTH,
            InboundEvent {
                kind: DEAUTHORIZE.to_owned(),
                path_params: BTreeMap::new(),
                headers: vec![(SIGNATURE_HEADER.to_owned(), signature.into_bytes())],
                raw_body: body.into(),
            },
        )
        .await
        .unwrap();

    let got = rows(&f, f.owner).await;
    assert_eq!(
        kinds(&got),
        [
            NotificationKind::AccountRevokedByPlatform,
            NotificationKind::AccountRevokedByPlatform
        ]
    );
    assert_eq!(names(&got), ["acct-1", "acct-2"]);
    assert!(rows(&f, other).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reminders_at_seven_days_and_one_day_once_each(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1"]).await;

    // Outside the window: nothing.
    set_reconnect_by(&f, g, Duration::days(10)).await;
    run(&f).await;
    assert!(rows(&f, f.owner).await.is_empty());

    set_reconnect_by(&f, g, Duration::days(6)).await;
    run(&f).await;
    run(&f).await;
    let got = rows(&f, f.owner).await;
    assert_eq!(got.len(), 1);
    assert_eq!(stage_of(&got[0]), Some(ReminderStage::SevenDays));
    assert_eq!(warned(&f, g).await, "7d");

    set_reconnect_by(&f, g, Duration::hours(12)).await;
    run(&f).await;
    run(&f).await;
    let got = rows(&f, f.owner).await;
    assert_eq!(got.len(), 2);
    assert!(
        got.iter()
            .any(|r| stage_of(r) == Some(ReminderStage::OneDay))
    );
    assert_eq!(warned(&f, g).await, "1d");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_first_seen_inside_one_day_gets_one_reminder(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1"]).await;
    set_reconnect_by(&f, g, Duration::hours(3)).await;

    run(&f).await;
    run(&f).await;

    let got = rows(&f, f.owner).await;
    assert_eq!(got.len(), 1);
    assert_eq!(stage_of(&got[0]), Some(ReminderStage::OneDay));
    assert_eq!(warned(&f, g).await, "1d");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconnect_by_passed_notifies_once_per_account(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1", "acct-2"]).await;
    set_reconnect_by(&f, g, Duration::minutes(-1)).await;

    run(&f).await;
    run(&f).await;

    let got = rows(&f, f.owner).await;
    assert_eq!(
        kinds(&got),
        [
            NotificationKind::ReconnectByPassed,
            NotificationKind::ReconnectByPassed
        ]
    );
    assert!(got.iter().all(|r| stage_of(r).is_none()));
    assert_eq!(names(&got), ["acct-1", "acct-2"]);
    assert_eq!(warned(&f, g).await, "expired");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegates_are_not_told_about_reconnects(pool: PgPool) {
    let f = fixture(&pool).await;
    let (g, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u"), "r", &["acct-1"]).await;
    let delegate = seed_user(&pool, "active").await;
    grant(&pool, f.owner, delegate, Level::Publish, AccountLimit::All).await;

    f.service
        .mark_needs_reconnect(&OwnerScope::own(f.owner), g, AuditContext::default())
        .await
        .unwrap();
    let (g2, _) = oauth_grant(&pool, f.owner, OAUTH, Some("u2"), "r2", &["acct-2"]).await;
    set_reconnect_by(&f, g2, Duration::minutes(-1)).await;
    run(&f).await;

    assert_eq!(rows(&f, f.owner).await.len(), 2);
    assert!(rows(&f, delegate).await.is_empty());
}
