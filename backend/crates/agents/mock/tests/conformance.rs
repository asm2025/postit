//! The conformance suite run against `mock` and `mock_direct`, with `MockScript` as the fake
//! platform's `Faults`.
//!
//! The suite holds a thread-local log capture, so these tests use `#[tokio::test]`'s default
//! current-thread runtime and live in their own test binary.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{TOKEN, account, context, fields, variant};
use postit_agent_kit::testkit::conformance::{
    ConformanceFixture, ConformanceReport, CredentialSamples, run_conformance,
};
use postit_agent_mock::MockAgent;
use postit_core::{
    Agent, DirectRecipient, DirectSendId, EmailBlock, EmailDocument, MediaAssetId, MediaItem,
    Variant,
};
use serde_json::Value;
use uuid::Uuid;

fn media(n: usize) -> Vec<MediaItem> {
    (0..n)
        .map(|_| MediaItem {
            asset: MediaAssetId::from_uuid(Uuid::now_v7()),
            alt_text: Some("a product photo".to_owned()),
        })
        .collect()
}

fn recipient(address: &str) -> DirectRecipient {
    DirectRecipient {
        send_id: DirectSendId::from_uuid(Uuid::now_v7()),
        address: address.to_owned(),
        merge_fields: BTreeMap::from([("first_name".to_owned(), "Sam".to_owned())]),
        locale: "en".to_owned(),
    }
}

/// A fixture for `agent` with one sample per declared format.
fn fixture(
    agent: MockAgent,
    samples: Vec<Variant>,
    recipients: Vec<DirectRecipient>,
) -> ConformanceFixture {
    let platform = agent.descriptor().platform.clone();
    let declared: Vec<_> = agent.descriptor().formats.iter().map(|f| f.id).collect();
    assert_eq!(
        declared.len(),
        samples.len(),
        "one sample per declared format: {declared:?}"
    );
    // A sample carries no media kind (the mock never reads the assets), so the only thing that
    // ties a sample to its format is the item count: it must fit that format's media rules.
    for (format, sample) in agent.descriptor().formats.iter().zip(&samples) {
        let count = u32::try_from(sample.media.len()).unwrap();
        assert!(
            (format.media.min_count..=format.media.max_count).contains(&count),
            "the {:?} sample has {count} media item(s)",
            format.id
        );
    }
    let faults = Arc::new(agent.script());
    let ctx = Arc::new(context(TOKEN));
    let check_context = ctx.check_context(Value::Null);
    ConformanceFixture {
        agent: Arc::new(agent),
        context: Arc::new(move || ctx.agent_context(Value::Null)),
        check_context,
        account: account(platform.as_str()),
        samples,
        credentials: Some(CredentialSamples {
            valid: fields("shop", TOKEN),
            invalid: fields("shop", "invalid"),
        }),
        recipients,
        secrets: vec![TOKEN.to_owned()],
        faults,
        deduplicates: true,
    }
}

fn assert_skipped(report: &ConformanceReport, skipped: &[&str]) {
    report.assert_passed();
    assert_eq!(report.skipped, skipped);
}

#[tokio::test]
async fn mock_broadcast_passes() {
    let text = variant(None);
    let image = Variant {
        media: media(2),
        ..variant(None)
    };
    let video = Variant {
        media: media(1),
        ..variant(None)
    };
    let report = run_conformance(fixture(
        MockAgent::broadcast(),
        vec![text, image, video],
        Vec::new(),
    ))
    .await;
    assert_skipped(&report, &["direct"]);
}

#[tokio::test]
async fn mock_direct_passes() {
    let email = Variant {
        target: account("mock_direct"),
        email: Some(EmailDocument {
            subject: "Our autumn sale".to_owned(),
            preheader: Some("Everything is 20% off".to_owned()),
            blocks: vec![EmailBlock::Text {
                text: "Hello {{first_name}}, everything is 20% off this week.".to_owned(),
            }],
        }),
        ..variant(None)
    };
    let recipients = vec![
        recipient("ana@example.com"),
        recipient("soft-bounce@example.com"),
        recipient("fail@example.com"),
    ];
    let report = run_conformance(fixture(MockAgent::direct(), vec![email], recipients)).await;
    assert_skipped(&report, &[]);
}

#[tokio::test]
async fn mock_oauth_passes() {
    let provider = url::Url::parse("https://provider.test/").unwrap();
    let agent = MockAgent::oauth(&provider);
    let declared = agent.descriptor().formats.len();
    assert_eq!(declared, 3);
    let image = Variant {
        target: account("mock_oauth"),
        media: media(2),
        ..variant(None)
    };
    let video = Variant {
        target: account("mock_oauth"),
        media: media(1),
        ..variant(None)
    };
    let text = Variant {
        target: account("mock_oauth"),
        ..variant(None)
    };
    let mut fx = fixture(agent, vec![text, image, video], Vec::new());
    let ctx = Arc::new(context(TOKEN));
    fx.check_context =
        ctx.check_context(serde_json::json!({"client_id": "c", "client_secret": "s"}));
    let report = run_conformance(fx).await;
    report.assert_passed();
}
