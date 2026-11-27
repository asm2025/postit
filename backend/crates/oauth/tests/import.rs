#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use common::{found, mock_agent, mock_oauth_agent, platforms};
use postit_agent_mock::VerifyScript;
use postit_core::{AccountImportId, PlatformId};
use postit_data::account_imports::{AccountImport, ImportState, ImportStop};
use postit_data::audit::AuditContext;
use postit_data::testkit::{callers, seed_user};
use postit_data::{OwnerScope, ScopeError};
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_oauth::{AccountError, AccountImportJob, AccountService, ImportRow, ImportStatus};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use tokio::io::{AsyncRead, ReadBuf};

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

async fn start(f: &Fixture, csv: &str, commit: bool) -> Result<AccountImport, AccountError> {
    f.service
        .start_import(
            &f.scope,
            &fake(),
            csv.as_bytes(),
            commit,
            AuditContext::default(),
        )
        .await
}

fn job(f: &Fixture, id: AccountImportId) -> AccountImportJob {
    AccountImportJob {
        import: id.as_uuid(),
        owner: f.scope.owner.as_uuid(),
    }
}

async fn run_job(f: &Fixture, id: AccountImportId) {
    f.service.run_import(job(f, id)).await.unwrap();
}

async fn get(f: &Fixture, id: AccountImportId) -> AccountImport {
    f.service.get_import(&f.scope, id).await.unwrap()
}

/// Stages `csv`, runs the job to the end, and returns the finished import.
async fn run(f: &Fixture, csv: &str, commit: bool) -> AccountImport {
    let started = start(f, csv, commit).await.unwrap();
    run_job(f, started.id).await;
    get(f, started.id).await
}

fn rows(import: &AccountImport) -> Vec<ImportRow> {
    serde_json::from_value(import.report.clone()).unwrap()
}

fn statuses(import: &AccountImport) -> Vec<ImportStatus> {
    rows(import).iter().map(|r| r.status).collect()
}

async fn count(pool: &PgPool, table: &'static str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn queued(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs WHERE job_type = 'account_import'")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn import_audits(pool: &PgPool, outcome: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
         WHERE kind = 'accounts_imported' AND details->>'outcome' = $1",
    )
    .bind(outcome)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A body that yields `data`, then fails with `kind`.
struct Cut {
    data: Vec<u8>,
    pos: usize,
    kind: io::ErrorKind,
}

impl Cut {
    fn new(data: &str, kind: io::ErrorKind) -> Self {
        Self {
            data: data.as_bytes().to_vec(),
            pos: 0,
            kind,
        }
    }
}

impl AsyncRead for Cut {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos >= self.data.len() {
            return Poll::Ready(Err(io::Error::from(self.kind)));
        }
        let n = buf.remaining().min(self.data.len() - self.pos);
        buf.put_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Poll::Ready(Ok(()))
    }
}

async fn start_cut(
    f: &Fixture,
    data: &str,
    kind: io::ErrorKind,
    commit: bool,
) -> Result<AccountImport, AccountError> {
    f.service
        .start_import(
            &f.scope,
            &fake(),
            Cut::new(data, kind),
            commit,
            AuditContext::default(),
        )
        .await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn start_import_stages_and_queues(pool: PgPool) {
    let f = fixture(&pool).await;

    let import = start(&f, "account_name,token\na,ok-1\na,ok-2\na,ok-3\n", true)
        .await
        .unwrap();

    assert_eq!(import.status, ImportState::Processing);
    assert_eq!(import.rows_total, 3);
    assert_eq!(import.rows_done, 0);
    assert_eq!((import.last_row, import.last_line), (3, 4));
    assert_eq!(import.stopped, None);
    assert_eq!(queued(&pool).await, 1);
    assert_eq!(count(&pool, "account_import_rows").await, 3);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn report_keeps_file_order_with_concurrency(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\na,ok-3\na,ok-4\n";

    let import = run(&f, csv, false).await;

    let numbers: Vec<u32> = rows(&import).iter().map(|r| r.row).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4]);
    assert!(statuses(&import).iter().all(|s| *s == ImportStatus::Ok));
    assert_eq!(import.status, ImportState::Completed);
    assert_eq!(import.rows_done, import.rows_total);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn run_import_dry_run_reports_without_connecting(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\n";

    let import = run(&f, csv, false).await;

    assert!(!import.commit);
    assert!(rows(&import).iter().all(|r| r.account_ids.is_empty()));
    assert_eq!(count(&pool, "account_grants").await, 0);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(count(&pool, "pending_connections").await, 0);
    assert_eq!(count(&pool, "account_import_rows").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn run_import_connects_valid_rows(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,\na,bad\na,none\na,down\n";

    let import = run(&f, csv, true).await;

    assert_eq!(
        statuses(&import),
        vec![
            ImportStatus::Ok,
            ImportStatus::InvalidFields,
            ImportStatus::InvalidCredentials,
            ImportStatus::NothingFound,
            ImportStatus::PlatformError,
        ]
    );
    let report = rows(&import);
    assert_eq!(report[0].account_ids.len(), 1);
    assert_eq!(report[1].violations, vec!["option_required".to_owned()]);
    assert_eq!(report[1].field.as_deref(), Some("token"));
    assert_eq!(report[2].field.as_deref(), Some("token"));
    assert_eq!(count(&pool, "connected_accounts").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 1);
    assert_eq!(import_audits(&pool, "completed").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn report_lines_follow_the_file(pool: PgPool) {
    let f = fixture(&pool).await;
    // The first cell of row 1 spans two lines, so row 2 starts on line 4.
    let csv = "account_name,token\n\"a\nb\",ok-1\na,ok-2\n";

    let import = run(&f, csv, false).await;

    let lines: Vec<(u32, u32)> = rows(&import).iter().map(|r| (r.row, r.line)).collect();
    assert_eq!(lines, vec![(1, 2), (2, 4)]);
    assert_eq!((import.last_row, import.last_line), (2, 4));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn malformed_row_is_reported_and_skipped(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na\na,ok-3\n";

    let import = run(&f, csv, false).await;

    let report = rows(&import);
    let seen: Vec<_> = report.iter().map(|r| (r.row, r.line, r.status)).collect();
    assert_eq!(
        seen,
        vec![
            (1, 2, ImportStatus::Ok),
            (2, 3, ImportStatus::InvalidFields),
            (3, 4, ImportStatus::Ok)
        ]
    );
    assert_eq!(report[1].violations, vec!["malformed_row"]);
    assert_eq!(import.stopped, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn repeated_remote_account_reports_already_connected_across_chunks(pool: PgPool) {
    // csv_concurrency 1 makes a chunk 4 rows, so rows 1 and 9 are three chunks apart.
    let mut settings = test_accounts_settings();
    settings.csv_concurrency = 1;
    let f = fixture_with(&pool, settings).await;
    let mut csv = String::from("account_name,token\na,dup\n");
    for n in 2..=8 {
        csv.push_str(&format!("a,ok-{n}\n"));
    }
    csv.push_str("a,dup\n");

    let import = run(&f, &csv, false).await;

    let report = statuses(&import);
    assert_eq!(report.len(), 9);
    assert_eq!(report[0], ImportStatus::Ok);
    assert_eq!(report[8], ImportStatus::AlreadyConnected);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_account_in_file_is_already_connected(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,dup\nb,dup\n";

    let dry = run(&f, csv, false).await;
    let committed = run(&f, csv, true).await;

    assert_eq!(
        statuses(&dry),
        [ImportStatus::Ok, ImportStatus::AlreadyConnected]
    );
    assert_eq!(
        statuses(&committed),
        [ImportStatus::Ok, ImportStatus::AlreadyConnected]
    );
    // One account, moved to the second row's credential.
    assert_eq!(count(&pool, "connected_accounts").await, 1);
    assert_eq!(count(&pool, "account_grants").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn crash_mid_chunk_resumes_without_loss_or_double_connect(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\na,ok-3\n";
    let started = start(&f, csv, true).await.unwrap();
    // The worker died after connecting the rows and before settling their chunk: the accounts
    // exist and every row is still staged. Connecting the same rows through a second import
    // leaves the first import in exactly that state.
    run(&f, csv, true).await;
    assert_eq!(count(&pool, "connected_accounts").await, 3);
    assert_eq!(count(&pool, "account_import_rows").await, 3);

    run_job(&f, started.id).await;

    let import = get(&f, started.id).await;
    assert_eq!(import.status, ImportState::Completed);
    assert_eq!(import.rows_done, import.rows_total);
    let numbers: Vec<u32> = rows(&import).iter().map(|r| r.row).collect();
    assert_eq!(numbers, vec![1, 2, 3], "every row is reported exactly once");
    assert!(
        statuses(&import)
            .iter()
            .all(|s| *s == ImportStatus::AlreadyConnected)
    );
    assert_eq!(count(&pool, "connected_accounts").await, 3);
    assert_eq!(
        count(&pool, "account_grants").await,
        3,
        "no duplicate grants"
    );
    assert_eq!(count(&pool, "account_import_rows").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn connected_elsewhere_row(pool: PgPool) {
    let f = fixture(&pool).await;
    let other = OwnerScope::own(seed_user(&pool, "active").await);
    let theirs = f
        .service
        .start_import(
            &other,
            &fake(),
            "account_name,token\na,x\n".as_bytes(),
            true,
            AuditContext::default(),
        )
        .await
        .unwrap();
    f.service
        .run_import(AccountImportJob {
            import: theirs.id.as_uuid(),
            owner: other.owner.as_uuid(),
        })
        .await
        .unwrap();

    let import = run(&f, "account_name,token\na,x\n", true).await;

    let report = rows(&import);
    assert_eq!(report[0].status, ImportStatus::ConnectedElsewhere);
    assert!(report[0].account_ids.is_empty());
    assert_eq!(count(&pool, "connected_accounts").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn display_name_applies_to_single_account_only(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "account_name,token,display_name\na,ok-1,Shop\na,multi,Ignored\n";

    let import = run(&f, csv, true).await;

    let report = rows(&import);
    assert!(!report[0].display_name_ignored);
    assert!(report[1].display_name_ignored);
    let names: Vec<String> =
        sqlx::query_scalar("SELECT display_name FROM connected_accounts ORDER BY display_name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(names, vec!["Account m1", "Account m2", "Shop"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_header_answers_before_any_import(pool: PgPool) {
    let f = fixture(&pool).await;

    for csv in [
        "account_name,token,color\na,ok-1,red\n",
        "account_name\na\n",
        "account_name,account_name,token\na,a,ok-1\n",
    ] {
        let result = start(&f, csv, true).await;
        assert!(
            matches!(result, Err(AccountError::CsvInvalid(_))),
            "{csv}: {result:?}"
        );
    }
    let Err(AccountError::CsvInvalid(message)) =
        start(&f, "account_name,token,color\na,ok-1,red\n", true).await
    else {
        panic!("expected csv_invalid");
    };
    assert!(message.contains("color"), "{message}");
    assert!(!message.contains("red"));
    assert_eq!(count(&pool, "account_imports").await, 0);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bom_crlf_and_spaced_headers_accepted(pool: PgPool) {
    let f = fixture(&pool).await;
    let csv = "\u{feff} account_name , token \r\na,ok-1\r\na,ok-2\r\n";

    let import = run(&f, csv, false).await;

    assert_eq!(import.rows_total, 2);
    assert!(statuses(&import).iter().all(|s| *s == ImportStatus::Ok));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn over_max_rows_stops_with_max_rows(pool: PgPool) {
    let mut settings = test_accounts_settings();
    settings.csv_max_rows = 2;
    let f = fixture_with(&pool, settings).await;
    let csv = "account_name,token\na,ok-1\na,ok-2\na,ok-3\n";

    // A dry run, so it is not held for a decision.
    let import = start(&f, csv, false).await.unwrap();

    assert_eq!(import.stopped, Some(ImportStop::MaxRows));
    assert_eq!(import.rows_total, 2);
    assert_eq!((import.last_row, import.last_line), (2, 3));
    assert_eq!(count(&pool, "account_import_rows").await, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn body_over_byte_limit_stops_with_max_bytes(pool: PgPool) {
    let f = fixture(&pool).await;
    // Two whole rows and half of a third.
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";

    let import = start_cut(&f, data, io::ErrorKind::FileTooLarge, false)
        .await
        .unwrap();

    assert_eq!(import.stopped, Some(ImportStop::MaxBytes));
    assert_eq!(import.rows_total, 2);
    assert_eq!((import.last_row, import.last_line), (2, 3));
    assert_eq!(import.status, ImportState::Processing);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unreadable_body_stops_with_unreadable(pool: PgPool) {
    let f = fixture(&pool).await;
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";

    let import = start_cut(&f, data, io::ErrorKind::ConnectionReset, false)
        .await
        .unwrap();

    assert_eq!(import.stopped, Some(ImportStop::Unreadable));
    assert_eq!(import.rows_total, 2);
    assert_eq!((import.last_row, import.last_line), (2, 3));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stopped_dry_run_runs_at_once(pool: PgPool) {
    let f = fixture(&pool).await;
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";

    let started = start_cut(&f, data, io::ErrorKind::FileTooLarge, false)
        .await
        .unwrap();

    assert_eq!(started.status, ImportState::Processing);
    assert_eq!(queued(&pool).await, 1);
    run_job(&f, started.id).await;
    let done = get(&f, started.id).await;
    assert_eq!(done.status, ImportState::Completed);
    assert_eq!(done.stopped, Some(ImportStop::MaxBytes));
    assert_eq!(rows(&done).len(), 2);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stopped_commit_waits_and_connects_nothing(pool: PgPool) {
    let f = fixture(&pool).await;
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";

    let started = start_cut(&f, data, io::ErrorKind::FileTooLarge, true)
        .await
        .unwrap();

    assert_eq!(started.status, ImportState::AwaitingDecision);
    assert_eq!(queued(&pool).await, 0);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    // A forged job changes nothing.
    run_job(&f, started.id).await;
    let after = get(&f, started.id).await;
    assert_eq!(after.status, ImportState::AwaitingDecision);
    assert_eq!(after.rows_done, 0);
    assert_eq!(count(&pool, "account_import_rows").await, 2);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn continue_connects_the_rows_read(pool: PgPool) {
    let f = fixture(&pool).await;
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";
    let started = start_cut(&f, data, io::ErrorKind::FileTooLarge, true)
        .await
        .unwrap();

    let resumed = f
        .service
        .continue_import(&f.scope, started.id)
        .await
        .unwrap();

    assert_eq!(resumed.status, ImportState::Processing);
    assert_eq!(queued(&pool).await, 1);
    run_job(&f, started.id).await;
    let done = get(&f, started.id).await;
    assert_eq!(done.status, ImportState::Completed);
    assert_eq!(statuses(&done), [ImportStatus::Ok, ImportStatus::Ok]);
    assert_eq!(count(&pool, "connected_accounts").await, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_deletes_staged_rows(pool: PgPool) {
    let f = fixture(&pool).await;
    let data = "account_name,token\na,ok-1\na,ok-2\na,ok-";
    let started = start_cut(&f, data, io::ErrorKind::FileTooLarge, true)
        .await
        .unwrap();

    let canceled = f
        .service
        .cancel_import(&f.scope, started.id, AuditContext::default())
        .await
        .unwrap();

    assert_eq!(canceled.status, ImportState::Canceled);
    assert_eq!(count(&pool, "account_import_rows").await, 0);
    assert_eq!(count(&pool, "connected_accounts").await, 0);
    assert_eq!(import_audits(&pool, "canceled").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn continue_or_cancel_refused_unless_waiting(pool: PgPool) {
    let f = fixture(&pool).await;
    let processing = start(&f, "account_name,token\na,ok-1\n", true)
        .await
        .unwrap();
    let completed = run(&f, "account_name,token\na,ok-2\n", true).await;
    let data = "account_name,token\na,ok-1\na,ok-";
    let waiting = start_cut(&f, data, io::ErrorKind::FileTooLarge, true)
        .await
        .unwrap();
    f.service
        .cancel_import(&f.scope, waiting.id, AuditContext::default())
        .await
        .unwrap();

    for id in [processing.id, completed.id, waiting.id] {
        assert!(matches!(
            f.service.continue_import(&f.scope, id).await,
            Err(AccountError::ImportNotWaiting)
        ));
        assert!(matches!(
            f.service
                .cancel_import(&f.scope, id, AuditContext::default())
                .await,
            Err(AccountError::ImportNotWaiting)
        ));
    }
    let unknown = AccountImportId::from_uuid(uuid::Uuid::now_v7());
    assert!(matches!(
        f.service.continue_import(&f.scope, unknown).await,
        Err(AccountError::NotFound)
    ));
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
        .start_import(
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
    assert_eq!(count(&pool, "account_imports").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_refused(pool: PgPool) {
    let f = fixture(&pool).await;
    let who = callers(&pool, f.scope.owner, &[]).await;
    let owned = start(&f, "account_name,token\na,ok-1\n", true)
        .await
        .unwrap();

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = f
            .service
            .start_import(
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
        assert!(matches!(
            f.service.get_import(scope, owned.id).await,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            f.service.continue_import(scope, owned.id).await,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            f.service
                .cancel_import(scope, owned.id, AuditContext::default())
                .await,
            Err(AccountError::Scope(ScopeError::OwnerOnly))
        ));
    }
}
