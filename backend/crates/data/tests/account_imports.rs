//! `account_imports` and its staged rows: the repository the account CSV import job runs on.

use chrono::{Duration, Utc};
use postit_core::{AccountImportId, PlatformId, UserId};
use postit_data::account_imports::{
    AccountImport, AccountImportRepo, ImportState, ImportStop, StagedAccountRow, StagedCells,
};
use postit_data::audit::AuditContext;
use postit_data::retention;
use postit_data::testkit::{Callers, callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use postit_vault::{SealContext, Vault};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn ok<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|e| unreachable!("{what}: {e}"))
}

fn platform() -> PlatformId {
    PlatformId::new("email").unwrap_or_else(|e| unreachable!("platform id: {e}"))
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    ok(pool.acquire().await, "acquire")
}

async fn seed(pool: &PgPool) -> (UserId, OwnerScope) {
    ok(
        sqlx::query("INSERT INTO platforms (id, display_name) VALUES ('email', 'Email')")
            .execute(pool)
            .await,
        "platform",
    );
    let owner = seed_user(pool, "active").await;
    (owner, OwnerScope::own(owner))
}

fn cells(name: &str, token: &str) -> StagedCells {
    StagedCells::Cells {
        cells: vec![
            ("name".to_owned(), name.to_owned()),
            ("token".to_owned(), token.to_owned()),
        ],
        display_name: Some(name.to_owned()),
    }
}

fn row(vault: &Vault, row_no: i32, line_no: i32, cells: &StagedCells) -> StagedAccountRow {
    let id = Uuid::now_v7();
    StagedAccountRow {
        id,
        row_no,
        line_no,
        data: ok(
            vault.seal(cells, &AccountImportRepo::seal_context(id)),
            "seal",
        ),
    }
}

/// The upload's request every test import is created with.
fn request() -> AuditContext {
    AuditContext {
        ip: Some(std::net::IpAddr::from([192, 0, 2, 7])),
        request_id: Some(Uuid::from_u128(7)),
    }
}

async fn create(conn: &mut PgConnection, scope: &OwnerScope, commit: bool) -> AccountImportId {
    let id = AccountImportId::from_uuid(Uuid::now_v7());
    ok(
        AccountImportRepo::create(conn, scope, id, &platform(), commit, request()).await,
        "create",
    );
    id
}

/// Claims and settles one row in its own transaction, as the job does.
async fn settle(pool: &PgPool, owner: UserId, id: AccountImportId, row_no: i32, seen: &[String]) {
    let mut tx = ok(pool.begin().await, "begin");
    let claimed = ok(
        AccountImportRepo::claim_row(&mut tx, owner, id, row_no).await,
        "claim",
    );
    assert!(claimed.is_some(), "row {row_no} is claimable");
    ok(
        AccountImportRepo::settle_row(&mut tx, owner, id, &json!({"row": row_no}), seen).await,
        "settle",
    );
    ok(tx.commit().await, "commit");
}

async fn staged(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    vault: &Vault,
    id: AccountImportId,
    count: i32,
) {
    let rows: Vec<_> = (1..=count)
        .map(|n| {
            row(
                vault,
                n,
                n + 1,
                &cells(&format!("n{n}"), &format!("tok-{n}")),
            )
        })
        .collect();
    ok(
        AccountImportRepo::stage(conn, scope, id, &rows).await,
        "stage",
    );
}

async fn row_count(pool: &PgPool, id: AccountImportId) -> i64 {
    ok(
        sqlx::query_scalar("SELECT count(*) FROM account_import_rows WHERE import_id = $1")
            .bind(id.as_uuid())
            .fetch_one(pool)
            .await,
        "row count",
    )
}

async fn age(pool: &PgPool, id: AccountImportId, hours: i64) {
    ok(
        sqlx::query("UPDATE account_imports SET updated_at = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(Utc::now() - Duration::hours(hours))
            .execute(pool)
            .await,
        "age",
    );
}

async fn get(conn: &mut PgConnection, scope: &OwnerScope, id: AccountImportId) -> AccountImport {
    ok(AccountImportRepo::get(conn, scope, id).await, "get")
}

#[sqlx::test]
async fn stage_chunk_settle_finish_round_trip(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, id, 3).await;
    ok(
        AccountImportRepo::set_total(&mut c, &scope, id, 3, None, 3, 4, ImportState::Processing)
            .await,
        "set_total",
    );

    let chunk = ok(
        AccountImportRepo::next_chunk(&mut c, owner, id, 2).await,
        "chunk",
    );
    assert_eq!(chunk.iter().map(|r| r.row_no).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(chunk.iter().map(|r| r.line_no).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(row_count(&pool, id).await, 3, "next_chunk deletes nothing");
    let opened = ok(
        vault.open(
            &chunk[0].data,
            &AccountImportRepo::seal_context(chunk[0].id),
        ),
        "open",
    );
    assert_eq!(opened, cells("n1", "tok-1"));

    settle(&pool, owner, id, 1, &["remote-1".to_owned()]).await;
    settle(&pool, owner, id, 2, &["remote-2".to_owned()]).await;
    let after = get(&mut c, &scope, id).await;
    assert_eq!(after.rows_done, 2);
    assert_eq!(after.report, json!([{"row": 1}, {"row": 2}]));
    assert_eq!(after.request, request(), "the upload's request is kept");
    assert_eq!(row_count(&pool, id).await, 1);
    assert_eq!(
        ok(
            AccountImportRepo::claim_row(&mut c, owner, id, 1).await,
            "claim again"
        ),
        None,
        "a settled row cannot be claimed again"
    );

    assert!(
        !ok(AccountImportRepo::finish(&mut c, owner, id).await, "finish"),
        "row 3 is still staged"
    );
    let mut tx = ok(pool.begin().await, "begin");
    assert_eq!(
        ok(
            AccountImportRepo::claim_row(&mut tx, owner, id, 3).await,
            "claim"
        ),
        Some(vec!["remote-1".to_owned(), "remote-2".to_owned()]),
        "a claim hands over the remote IDs settled so far"
    );
    drop(tx);
    assert_eq!(
        row_count(&pool, id).await,
        1,
        "a dropped claim keeps the row"
    );
    settle(&pool, owner, id, 3, &[]).await;

    assert!(ok(
        AccountImportRepo::finish(&mut c, owner, id).await,
        "finish"
    ));
    assert_eq!(get(&mut c, &scope, id).await.status, ImportState::Completed);
    assert!(
        !ok(AccountImportRepo::finish(&mut c, owner, id).await, "finish"),
        "an import closes once"
    );
}

#[sqlx::test]
async fn claim_row_refuses_an_import_that_is_not_processing(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, id, 2).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            id,
            2,
            Some(ImportStop::MaxRows),
            2,
            3,
            ImportState::AwaitingDecision,
        )
        .await,
        "set_total",
    );

    assert_eq!(
        ok(
            AccountImportRepo::claim_row(&mut c, owner, id, 1).await,
            "claim"
        ),
        None
    );
    assert_eq!(
        row_count(&pool, id).await,
        2,
        "a waiting import keeps its rows"
    );
}

#[sqlx::test]
async fn unsettled_chunk_survives_a_crash(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, id, 3).await;

    let mut tx = ok(pool.begin().await, "begin");
    let first = ok(
        AccountImportRepo::next_chunk(&mut tx, owner, id, 2).await,
        "chunk",
    );
    ok(tx.rollback().await, "rollback");
    let second = ok(
        AccountImportRepo::next_chunk(&mut c, owner, id, 2).await,
        "chunk 2",
    );
    assert_eq!(
        first.iter().map(|r| r.row_no).collect::<Vec<_>>(),
        second.iter().map(|r| r.row_no).collect::<Vec<_>>()
    );
    assert_eq!(row_count(&pool, id).await, 3);
}

#[sqlx::test]
async fn rows_are_sealed_and_open_only_for_their_row(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, id, 2).await;

    let raw: Vec<Vec<u8>> = ok(
        sqlx::query_scalar("SELECT data FROM account_import_rows WHERE import_id = $1")
            .bind(id.as_uuid())
            .fetch_all(&pool)
            .await,
        "raw",
    );
    assert_eq!(raw.len(), 2);
    for bytes in &raw {
        assert!(
            !bytes.windows(5).any(|w| w == b"tok-1" || w == b"tok-2"),
            "a staged cell is visible in the stored bytes"
        );
    }
    let chunk = ok(
        AccountImportRepo::next_chunk(&mut c, owner, id, 2).await,
        "chunk",
    );
    let other = SealContext {
        table: "account_import_rows",
        column: "data",
        row: chunk[1].id,
    };
    assert!(vault.open(&chunk[0].data, &other).is_err());
}

#[sqlx::test]
async fn set_total_records_the_stop(pool: PgPool) {
    let (_owner, scope) = seed(&pool).await;
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            id,
            7,
            Some(ImportStop::MaxBytes),
            7,
            9,
            ImportState::AwaitingDecision,
        )
        .await,
        "set_total",
    );
    let got = get(&mut c, &scope, id).await;
    assert_eq!(got.rows_total, 7);
    assert_eq!(got.stopped, Some(ImportStop::MaxBytes));
    assert_eq!((got.last_row, got.last_line), (7, 9));
    assert_eq!(got.status, ImportState::AwaitingDecision);
    assert!(got.commit);
    assert_eq!(got.platform, platform());
}

#[sqlx::test]
async fn resume_and_cancel_only_from_awaiting_decision(pool: PgPool) {
    let (_owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;

    // A waiting import resumes and keeps its rows.
    let a = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, a, 2).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            a,
            2,
            Some(ImportStop::MaxRows),
            2,
            3,
            ImportState::AwaitingDecision,
        )
        .await,
        "wait a",
    );
    assert!(ok(
        AccountImportRepo::resume(&mut c, &scope, a).await,
        "resume"
    ));
    assert_eq!(get(&mut c, &scope, a).await.status, ImportState::Processing);
    assert_eq!(row_count(&pool, a).await, 2);
    assert!(!ok(
        AccountImportRepo::resume(&mut c, &scope, a).await,
        "again"
    ));
    assert!(!ok(
        AccountImportRepo::cancel(&mut c, &scope, a).await,
        "cancel a"
    ));
    assert_eq!(get(&mut c, &scope, a).await.status, ImportState::Processing);

    // Another cancels and loses its rows.
    let b = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, b, 2).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            b,
            2,
            Some(ImportStop::MaxRows),
            2,
            3,
            ImportState::AwaitingDecision,
        )
        .await,
        "wait b",
    );
    assert!(ok(
        AccountImportRepo::cancel(&mut c, &scope, b).await,
        "cancel"
    ));
    assert_eq!(get(&mut c, &scope, b).await.status, ImportState::Canceled);
    assert_eq!(row_count(&pool, b).await, 0);
    assert!(!ok(
        AccountImportRepo::cancel(&mut c, &scope, b).await,
        "again"
    ));
    assert!(!ok(
        AccountImportRepo::resume(&mut c, &scope, b).await,
        "resume b"
    ));

    // A completed import is neither.
    let d = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::finish(&mut c, scope.owner, d).await,
        "finish",
    );
    assert!(!ok(
        AccountImportRepo::resume(&mut c, &scope, d).await,
        "resume d"
    ));
    assert!(!ok(
        AccountImportRepo::cancel(&mut c, &scope, d).await,
        "cancel d"
    ));
    assert_eq!(get(&mut c, &scope, d).await.status, ImportState::Completed);
}

#[sqlx::test]
async fn get_refuses_another_owner(pool: PgPool) {
    let (_owner, scope) = seed(&pool).await;
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, false).await;
    assert!(matches!(
        AccountImportRepo::get(&mut c, &stranger, id).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn delegate_cannot_create_resume_or_cancel(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let c: Callers = callers(&pool, owner, &[]).await;
    let mut conn = conn(&pool).await;
    let id = create(&mut conn, &scope, true).await;
    let refused =
        |r: Result<(), DataError>| matches!(r, Err(DataError::Scope(ScopeError::OwnerOnly)));
    for delegate in [&c.view, &c.edit, &c.publish, &c.limited] {
        assert!(refused(
            AccountImportRepo::create(
                &mut conn,
                delegate,
                AccountImportId::from_uuid(Uuid::now_v7()),
                &platform(),
                true,
                AuditContext::default(),
            )
            .await
        ));
        assert!(refused(
            AccountImportRepo::resume(&mut conn, delegate, id)
                .await
                .map(|_| ())
        ));
        assert!(refused(
            AccountImportRepo::cancel(&mut conn, delegate, id)
                .await
                .map(|_| ())
        ));
        assert!(refused(
            AccountImportRepo::get(&mut conn, delegate, id)
                .await
                .map(|_| ())
        ));
        assert!(refused(
            AccountImportRepo::stage(&mut conn, delegate, id, &[]).await
        ));
        assert!(refused(
            AccountImportRepo::set_total(
                &mut conn,
                delegate,
                id,
                1,
                None,
                1,
                2,
                ImportState::Processing
            )
            .await
        ));
    }
}

#[sqlx::test]
async fn stale_import_fails_and_loses_rows(pool: PgPool) {
    let (_owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let processing = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, processing, 2).await;
    let waiting = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, waiting, 2).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            waiting,
            2,
            Some(ImportStop::MaxRows),
            2,
            3,
            ImportState::AwaitingDecision,
        )
        .await,
        "wait",
    );
    let fresh = create(&mut c, &scope, true).await;
    staged(&mut c, &scope, &vault, fresh, 2).await;
    age(&pool, processing, 25).await;
    age(&pool, waiting, 25).await;

    let failed = ok(
        retention::fail_stale_account_imports(&mut c, Utc::now() - Duration::hours(24)).await,
        "sweep",
    );
    assert_eq!(failed, 2);
    for id in [processing, waiting] {
        assert_eq!(get(&mut c, &scope, id).await.status, ImportState::Failed);
        assert_eq!(row_count(&pool, id).await, 0);
    }
    assert_eq!(
        get(&mut c, &scope, fresh).await.status,
        ImportState::Processing
    );
    assert_eq!(row_count(&pool, fresh).await, 2);
}

#[sqlx::test]
async fn purge_deletes_finished_imports_only(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let mut c = conn(&pool).await;
    let completed = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::finish(&mut c, owner, completed).await,
        "finish",
    );
    let failed = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::abandon(&mut c, owner, failed).await,
        "abandon",
    );
    let canceled = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            canceled,
            0,
            Some(ImportStop::MaxRows),
            0,
            0,
            ImportState::AwaitingDecision,
        )
        .await,
        "wait",
    );
    assert!(ok(
        AccountImportRepo::cancel(&mut c, &scope, canceled).await,
        "cancel"
    ));
    let processing = create(&mut c, &scope, true).await;
    let waiting = create(&mut c, &scope, true).await;
    ok(
        AccountImportRepo::set_total(
            &mut c,
            &scope,
            waiting,
            0,
            Some(ImportStop::MaxRows),
            0,
            0,
            ImportState::AwaitingDecision,
        )
        .await,
        "wait",
    );
    ok(
        sqlx::query("UPDATE account_imports SET created_at = now() - interval '40 days'")
            .execute(&pool)
            .await,
        "backdate",
    );

    let purged = ok(
        retention::purge_account_imports(&mut c, Utc::now() - Duration::days(30)).await,
        "purge",
    );
    assert_eq!(purged, 3);
    for id in [completed, failed, canceled] {
        assert!(matches!(
            AccountImportRepo::get(&mut c, &scope, id).await,
            Err(DataError::NotFound)
        ));
    }
    for id in [processing, waiting] {
        get(&mut c, &scope, id).await;
    }
}

#[sqlx::test]
async fn settle_row_keeps_entries_in_order(pool: PgPool) {
    let (owner, scope) = seed(&pool).await;
    let vault = vault();
    let mut c = conn(&pool).await;
    let id = create(&mut c, &scope, false).await;
    staged(&mut c, &scope, &vault, id, 4).await;
    for row_no in 1..=4 {
        settle(&pool, owner, id, row_no, &[]).await;
    }
    let got = get(&mut c, &scope, id).await;
    assert_eq!(got.rows_done, 4);
    assert_eq!(
        got.report,
        json!([{"row": 1}, {"row": 2}, {"row": 3}, {"row": 4}])
    );
    assert_eq!(row_count(&pool, id).await, 0);
}
