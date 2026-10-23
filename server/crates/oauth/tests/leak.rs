#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{SECRET, platform, platforms};
use postit_core::DiscoveredAccount;
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::testkit::seed_user;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_oauth::testkit::{
    Script, TestCredentialAgent, VerifyScript, account_service, test_accounts_settings,
};
use serde_json::{Map, Value, json};
use sqlx::PgPool;

fn values() -> Map<String, Value> {
    let Value::Object(map) = json!({"server_url": "https://example.test", "token": SECRET}) else {
        unreachable!()
    };
    map
}

/// The secret must not appear in logs, returned values, or the audit trail, on the success
/// path or on any failure path.
#[sqlx::test(migrations = "../data/migrations")]
async fn secrets_never_leak(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    platforms(&pool, &["fake"]).await;
    let agent = TestCredentialAgent::new("fake");
    agent.set_script(Script {
        verify: VerifyScript::Accounts(vec![DiscoveredAccount {
            remote_id: "a".to_owned(),
            display_name: "A".to_owned(),
            handle: None,
            avatar_url: None,
        }]),
        ..Script::default()
    });
    let service = account_service(pool.clone(), vec![agent.clone()], test_accounts_settings());
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);

    let check = service
        .start_credentials(&scope, &platform("fake"), values())
        .await
        .unwrap();
    let (connect_id, _) = check.pending.as_ref().unwrap();
    let connect_text = connect_id.as_str().to_owned();
    let confirmed = service
        .confirm(&scope, &connect_text, None, AuditContext::default())
        .await
        .unwrap();

    agent.set_script(Script {
        verify: VerifyScript::Invalid(Some("token".to_owned())),
        ..Script::default()
    });
    let refused = service
        .start_credentials(&scope, &platform("fake"), values())
        .await
        .unwrap_err();
    agent.set_script(Script {
        verify: VerifyScript::Fail,
        ..Script::default()
    });
    let failed = service
        .start_credentials(&scope, &platform("fake"), values())
        .await
        .unwrap_err();

    let audit: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    let logs = logs.text();
    assert_no_leak(
        &[SECRET],
        &[
            ("logs", &logs),
            ("check", &format!("{check:?}")),
            ("confirmed", &format!("{confirmed:?}")),
            ("refused", &format!("{refused} {refused:?}")),
            ("failed", &format!("{failed} {failed:?}")),
            ("audit", &format!("{audit:?}")),
        ],
    );
    // The connect ID is a secret too: it is shown once and never logged or audited.
    assert_no_leak(
        &[connect_text.as_str()],
        &[
            ("logs", &logs),
            ("confirmed", &format!("{confirmed:?}")),
            ("audit", &format!("{audit:?}")),
        ],
    );
    assert!(!format!("{:?}", check.pending.as_ref().unwrap().0).contains(&connect_text));
}

/// A secret pasted into a column that does not look secret (`display_name`) must not reach the
/// report, the logs, or the audit trail.
#[sqlx::test(migrations = "../data/migrations")]
async fn import_report_never_contains_secret(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    platforms(&pool, &["fake"]).await;
    let agent = TestCredentialAgent::new("fake");
    agent.set_script(Script {
        verify: VerifyScript::Accounts(vec![DiscoveredAccount {
            remote_id: "a".to_owned(),
            display_name: "A".to_owned(),
            handle: None,
            avatar_url: None,
        }]),
        ..Script::default()
    });
    let service = account_service(pool.clone(), vec![agent], test_accounts_settings());
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let csv = format!(
        "server_url,token,display_name\nhttps://a.test,{SECRET},{SECRET}\nhttps://a.test,,{SECRET}\n"
    );

    let report = service
        .import_csv(
            &scope,
            &platform("fake"),
            csv.as_bytes(),
            true,
            AuditContext::default(),
        )
        .await
        .unwrap();

    let audit: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    let names: Vec<String> = sqlx::query_scalar("SELECT display_name FROM connected_accounts")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(report.rows.len(), 2);
    assert_no_leak(
        &[SECRET],
        &[
            ("report", &serde_json::to_string(&report).unwrap()),
            ("report debug", &format!("{report:?}")),
            ("logs", &logs.text()),
            ("audit", &format!("{audit:?}")),
        ],
    );
    // The secret in `display_name` is the user's own name for the account; it is stored there
    // (the user typed it), but nowhere else.
    assert_eq!(names, vec![SECRET.to_owned()]);
}
