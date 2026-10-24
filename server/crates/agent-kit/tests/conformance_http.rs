//! The conformance suite run against a small HTTP agent on wiremock, proving an agent that talks
//! to a real HTTP API can pass it, and that the suite catches each kind of broken agent.
#![cfg(feature = "testkit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use postit_agent_kit::testkit::conformance::{ConformanceReport, run_conformance};
use support::echo::Defects;

async fn run(defects: Defects) -> ConformanceReport {
    run_conformance(support::echo_fixture(defects).await).await
}

fn assert_only(report: &ConformanceReport, check: &str) {
    let failed: Vec<&str> = report.failures.iter().map(|f| f.check).collect();
    assert_eq!(failed, [check], "{:#?}", report.failures);
}

#[tokio::test]
async fn echo_agent_passes_conformance() {
    let report = run(Defects::default()).await;
    report.assert_passed();
    assert_eq!(report.skipped, ["direct"]);
}

#[tokio::test]
async fn catches_capability_mismatch() {
    let report = run(Defects {
        undeclared_commenter: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "capabilities");
}

#[tokio::test]
async fn catches_dry_run_call() {
    let report = run(Defects {
        dry_run_calls: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "dry_run");
}

#[tokio::test]
async fn catches_secret_in_error() {
    let report = run(Defects {
        secret_in_error: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "redaction");
}

#[tokio::test]
async fn catches_missing_english() {
    let report = run(Defects {
        missing_english: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "messages");
}

#[tokio::test]
async fn catches_retryable_after_send() {
    let report = run(Defects {
        retryable_after_send: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "error_mapping");
}

#[test]
#[should_panic(expected = "capabilities")]
fn assert_passed_lists_failures() {
    use postit_agent_kit::testkit::conformance::CheckFailure;
    ConformanceReport {
        failures: vec![
            CheckFailure {
                check: "capabilities",
                detail: "declared [Commenter], exposed []".to_owned(),
            },
            CheckFailure {
                check: "dry_run",
                detail: "made 1 remote call".to_owned(),
            },
        ],
        skipped: Vec::new(),
    }
    .assert_passed();
}
