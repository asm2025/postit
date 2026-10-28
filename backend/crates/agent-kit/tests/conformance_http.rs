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

#[tokio::test]
async fn catches_uncounted_fault_requests() {
    let report = run(Defects {
        uncounted_faults: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "error_mapping");
    assert!(
        report.failures[0]
            .detail
            .contains("did not count the faulted request"),
        "{:#?}",
        report.failures
    );
}

#[tokio::test]
async fn catches_permanent_class_with_nothing_sent() {
    let report = run(Defects {
        uncounted_faults: true,
        permanent_failures: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "error_mapping");
    for fault in ["ServerError", "LostBeforeAccept"] {
        let want = format!("{fault} with nothing sent: got Permanent, want Retryable or Ambiguous");
        assert!(
            report.failures[0].detail.contains(&want),
            "{:#?}",
            report.failures
        );
    }
}

#[tokio::test]
async fn catches_frozen_counters() {
    let report = run(Defects {
        frozen_counters: true,
        ..Defects::default()
    })
    .await;
    let failed: Vec<&str> = report.failures.iter().map(|f| f.check).collect();
    assert_eq!(
        failed,
        ["error_mapping", "idempotency"],
        "{:#?}",
        report.failures
    );
    assert!(
        report.failures[1]
            .detail
            .contains("Faults counters do not move"),
        "{:#?}",
        report.failures
    );
}

fn assert_only_detail(report: &ConformanceReport, check: &str, detail: &str) {
    assert_only(report, check);
    assert!(
        report.failures[0].detail.contains(detail),
        "{:#?}",
        report.failures
    );
}

#[tokio::test]
async fn catches_double_post_for_a_repeated_key() {
    let report = run(Defects {
        ignores_key: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "idempotency", "created a second post");
}

#[tokio::test]
async fn catches_double_post_without_remote_ids() {
    let report = run(Defects {
        ignores_key: true,
        no_remote_id: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "idempotency", "no remote ID");
}

#[tokio::test]
async fn skips_repeat_key_for_a_platform_that_does_not_deduplicate() {
    let mut fixture = support::echo_fixture(Defects {
        ignores_key: true,
        ..Defects::default()
    })
    .await;
    fixture.deduplicates = false;
    let report = run_conformance(fixture).await;
    report.assert_passed();
    assert_eq!(report.skipped, ["idempotency", "direct"]);
}

#[tokio::test]
async fn catches_ignored_cancellation() {
    let report = run(Defects {
        ignores_cancel: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "cancellation", "mutating call");
}

#[tokio::test]
async fn catches_accepted_invalid_credentials() {
    let report = run(Defects {
        accepts_invalid_credentials: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "credentials", "invalid credentials were accepted");
}

#[tokio::test]
async fn catches_rejected_samples() {
    let report = run(Defects {
        rejects_samples: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "validate", "violation");
}

#[tokio::test]
async fn catches_dropped_recipient() {
    let report = run(Defects {
        drops_recipient: true,
        ..Defects::default()
    })
    .await;
    assert_only_detail(&report, "direct", "1 result(s) for 2 recipient(s)");
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

#[tokio::test]
async fn conformance_flags_oauth_agent_without_client_fields() {
    let report = run(Defects {
        oauth_without_client_fields: true,
        ..Defects::default()
    })
    .await;
    assert_only(&report, "oauth_config_fields");
}
