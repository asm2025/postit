#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{connect_all, found, mock_agent, mock_oauth_agent, platforms};
use postit_agent_mock::VerifyScript;
use postit_core::{AccountId, PlatformId};
use postit_data::audit::AuditContext;
use postit_data::testkit::{callers, seed_user};
use postit_data::{OwnerScope, ScopeError};
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountError, AccountService, ImportReport, ImportStatus};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use uuid::Uuid;

/// Decides a row's outcome from its token: `ok-N` reaches account `acct-N` after a delay that
/// shrinks with N (so earlier rows finish last); `multi` reaches two accounts; `bad` is
/// rejected; `none` reaches nothing; `dup` reaches `shared`; `x` reaches `elsewhere`.
fn row_script() -> VerifyScript {
    VerifyScript::ByFields(Arc::new(|fields| {
        let token = fields
            .get_secret("token")
            .unwrap()
            .expose_secret()
            .to_owned();
        let outcome =
            |ids: &[&str]| VerifyScript::Accounts(ids.iter().map(|id| found(id)).collect());
        if let Some(n) = token.strip_prefix("ok-") {
            let n: u64 = n.parse().unwrap();
            return (
                Duration::from_millis(20 * (6 - n.min(5))),
                outcome(&[&format!("acct-{n}")]),
            );
        }
        match token.as_str() {
            "multi" => (Duration::ZERO, outcome(&["m1", "m2"])),
            "bad" => (
                Duration::ZERO,
                VerifyScript::Invalid(Some("token".to_owned())),
            ),
            "none" => (Duration::ZERO, outcome(&[])),
            "dup" => (Duration::ZERO, outcome(&["shared"])),
            "x" => (Duration::ZERO, outcome(&["elsewhere"])),
            "down" => (Duration::ZERO, VerifyScript::Fail),
            _ => (Duration::ZERO, outcome(&["other"])),
        }
    }))
}

struct Fixture {
    service: AccountService,
    scope: OwnerScope,
}

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, test_accounts_settings()).await
}

async fn fixture_with(pool: &PgPool, settings: postit_config::AccountsSettings) -> Fixture {
    platforms(pool, &["fake"]).await;
    let (agent, script) = mock_agent("fake");
    script.set_verify(row_script());
    let service = account_service(pool.clone(), vec![agent], settings);
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        scope: OwnerScope::own(owner),
    }
}

fn fake() -> PlatformId {
    PlatformId::new("fake").unwrap()
}

async fn run(f: &Fixture, csv: &str, commit: bool) -> Result<ImportReport, AccountError> {
    f.service
        .import_csv(
            &f.scope,
            &fake(),
            csv.as_bytes(),
            commit,
            AuditContext::default(),
        )
        .await
}

async fn count(pool: &PgPool, table: &'static str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn report_keeps_file_order_with_concurrency(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\na,ok-3\na,ok-4\n";

    let report = run(&f, csv, false).await.unwrap();

    let rows: Vec<u32> = report.rows.iter().map(|r| r.row).collect();
    assert_eq!(rows, vec![1, 2, 3, 4]);
    assert!(report.rows.iter().all(|r| r.status == ImportStatus::Ok));
    assert_eq!(report.counts[&ImportStatus::Ok], 4);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dry_run_reports_without_creating(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\n";

    let report = run(&f, csv, false).await.unwrap();

    assert!(!report.commit);
    assert!(report.rows.iter().all(|r| r.account_ids.is_empty()));
    assert_eq!(count(&pool, "account_grants").await, 0);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(count(&pool, "pending_connections").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn commit_creates_valid_rows_and_reports_bad_ones(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,\na,bad\na,none\na,down\n";

    let report = run(&f, csv, true).await.unwrap();

    let statuses: Vec<ImportStatus> = report.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        vec![
            ImportStatus::Ok,
            ImportStatus::InvalidFields,
            ImportStatus::InvalidCredentials,
            ImportStatus::NothingFound,
            ImportStatus::PlatformError,
        ]
    );
    assert_eq!(report.rows[0].account_ids.len(), 1);
    assert_eq!(
        report.rows[1].violations,
        vec!["option_required".to_owned()]
    );
    assert_eq!(report.rows[1].field.as_deref(), Some("token"));
    assert_eq!(report.rows[2].field.as_deref(), Some("token"));
    assert_eq!(count(&pool, "connected_accounts").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 1);
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE kind = 'accounts_imported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(events, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_account_in_file_is_already_connected(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,dup\nb,dup\n";

    let dry = run(&f, csv, false).await.unwrap();
    let committed = run(&f, csv, true).await.unwrap();

    assert_eq!(dry.rows[0].status, ImportStatus::Ok);
    assert_eq!(dry.rows[1].status, ImportStatus::AlreadyConnected);
    assert_eq!(committed.rows[0].status, ImportStatus::Ok);
    assert_eq!(committed.rows[1].status, ImportStatus::AlreadyConnected);
    // One account, moved to the second row's credential.
    assert_eq!(count(&pool, "connected_accounts").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn connected_elsewhere_row(pool: PgPool) {
    let f = fixture(&pool).await;
    let other = OwnerScope::own(seed_user(&pool, "active").await);
    f.service
        .import_csv(
            &other,
            &fake(),
            "account_name,token\na,x\n".as_bytes(),
            true,
            AuditContext::default(),
        )
        .await
        .unwrap();

    let report = run(&f, "account_name,token\na,x\n", true).await.unwrap();

    assert_eq!(report.rows[0].status, ImportStatus::ConnectedElsewhere);
    assert!(report.rows[0].account_ids.is_empty());
    assert_eq!(count(&pool, "connected_accounts").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn display_name_applies_to_single_account_only(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token,display_name\na,ok-1,Shop\na,multi,Ignored\n";

    let report = run(&f, csv, true).await.unwrap();

    assert!(!report.rows[0].display_name_ignored);
    assert!(report.rows[1].display_name_ignored);
    let names: Vec<String> =
        sqlx::query_scalar("SELECT display_name FROM connected_accounts ORDER BY display_name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(names, vec!["Account m1", "Account m2", "Shop"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_column_rejected(pool: PgPool) {
    let f = fixture(&pool).await;

    let result = run(&f, "account_name,token,color\na,ok-1,red\n", true).await;

    let Err(AccountError::CsvInvalid(message)) = result else {
        panic!("expected csv_invalid");
    };
    assert!(message.contains("color"), "{message}");
    assert!(!message.contains("red"));
    assert_eq!(count(&pool, "connected_accounts").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn missing_required_column_rejected(pool: PgPool) {
    let f = fixture(&pool).await;

    let result = run(&f, "account_name\na\n", true).await;

    let Err(AccountError::CsvInvalid(message)) = result else {
        panic!("expected csv_invalid");
    };
    assert!(message.contains("token"), "{message}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rows_past_the_limit_are_not_read(pool: PgPool) {
    let mut settings = test_accounts_settings();
    settings.csv_max_rows = 2;
    let f = fixture_with(&pool, settings).await;
    let csv = "account_name,token
a,ok-1
a,ok-2
a,ok-3
";

    let report = run(&f, csv, true).await.unwrap();

    assert!(report.truncated);
    assert_eq!(report.rows.len(), 2);
    assert!(report.rows.iter().all(|r| r.status == ImportStatus::Ok));
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_malformed_row_is_skipped_and_the_rest_continue(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token
a,ok-1
a
a,ok-3
";

    let report = run(&f, csv, false).await.unwrap();

    assert!(!report.truncated);
    let statuses: Vec<_> = report.rows.iter().map(|r| (r.row, r.status)).collect();
    assert_eq!(
        statuses,
        vec![
            (1, ImportStatus::Ok),
            (2, ImportStatus::InvalidFields),
            (3, ImportStatus::Ok)
        ]
    );
    assert_eq!(report.rows[1].violations, vec!["malformed_row"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bom_crlf_and_spaced_headers_accepted(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "\u{feff} account_name , token \r\na,ok-1\r\na,ok-2\r\n";

    let report = run(&f, csv, false).await.unwrap();

    assert_eq!(report.rows.len(), 2);
    assert!(report.rows.iter().all(|r| r.status == ImportStatus::Ok));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_platform_refused(pool: PgPool) {
    platforms(&pool, &["fake"]).await;
    let service = account_service(
        pool.clone(),
        vec![mock_oauth_agent("fake")],
        test_accounts_settings(),
    );
    let owner = OwnerScope::own(seed_user(&pool, "active").await);

    let result = service
        .import_csv(
            &owner,
            &fake(),
            "account_name,token\n".as_bytes(),
            false,
            AuditContext::default(),
        )
        .await;

    assert!(matches!(
        result,
        Err(AccountError::ConnectMethodUnsupported | AccountError::PlatformUnavailable)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_owner_only(pool: PgPool) {
    let f = fixture(&pool).await;
    let who = callers(&pool, f.scope.owner, &[AccountId::from(Uuid::now_v7())]).await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .import_csv(
                scope,
                &fake(),
                "account_name,token\na,ok-1\n".as_bytes(),
                true,
                AuditContext::default(),
            )
            .await;
        assert!(matches!(
            result,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let _ = connect_all;
}
