//! The conformance suite run against `mock` and `mock_direct`, with `MockScript` as the fake
//! platform's `Faults`.
//!
//! The suite holds a thread-local log capture, so these tests use `#[tokio::test]`'s default
//! current-thread runtime and live in their own test binary.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;

use common::{TOKEN, account, context, fields, variant};
use postit_agent_kit::testkit::conformance::{
    ConformanceFixture, ConformanceReport, CredentialSamples, run_conformance,
};
use postit_agent_mock::MockAgent;
use postit_core::{
    Agent, ByteStream, DirectRecipient, DirectSendId, EmailBlock, EmailDocument, MediaAsset,
    MediaItem, MediaKind, SystemClock, Variant,
};
use postit_data::OwnerScope;
use postit_data::testkit::seed_user;
use postit_media::testkit::{media_service, media_settings};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// A real video, probed and stored like any upload.
const VIDEO: &[u8] = include_bytes!("../../../media/tests/fixtures/landscape.mp4");

/// Real assets, uploaded through `MediaService` into a temp directory backed by
/// `FsMediaStore`, so the suite hands `validate` what the engine would.
struct Library {
    _dir: tempfile::TempDir,
    images: Vec<MediaAsset>,
    video: MediaAsset,
}

impl Library {
    /// Every asset, in upload order.
    fn all(&self) -> Vec<MediaAsset> {
        let mut all = self.images.clone();
        all.push(self.video.clone());
        all
    }
}

fn whole(bytes: &[u8]) -> ByteStream {
    futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes))]).boxed()
}

fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([10, 200, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

async fn library(pool: &PgPool) -> Library {
    let dir = tempfile::tempdir().unwrap();
    let service = media_service(
        pool.clone(),
        media_settings(dir.path()),
        Arc::new(SystemClock),
    )
    .unwrap();
    let owner = seed_user(pool, "active").await;
    let scope = OwnerScope::own(owner);
    let image = png();
    let mut ids = Vec::new();
    for name in ["one.png", "two.png"] {
        let media = service
            .upload(&scope, name, image.len() as u64, whole(&image))
            .await
            .unwrap();
        ids.push(media.id);
    }
    let video = service
        .upload(&scope, "clip.mp4", VIDEO.len() as u64, whole(VIDEO))
        .await
        .unwrap();
    ids.push(video.id);
    let mut assets = service.assets(&scope, &ids).await.unwrap();
    let video = assets.pop().unwrap();
    assert_eq!(video.kind, MediaKind::Video);
    assert!(assets.iter().all(|a| a.kind == MediaKind::Image));
    Library {
        _dir: dir,
        images: assets,
        video,
    }
}

fn items(assets: &[MediaAsset]) -> Vec<MediaItem> {
    assets
        .iter()
        .map(|asset| MediaItem {
            asset: asset.id,
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
    assets: Vec<MediaAsset>,
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
        assets,
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

#[sqlx::test(migrations = "../../data/migrations")]
async fn mock_broadcast_passes(pool: PgPool) {
    let library = library(&pool).await;
    let text = variant(None);
    let image = Variant {
        media: items(&library.images),
        ..variant(None)
    };
    let video = Variant {
        media: items(std::slice::from_ref(&library.video)),
        ..variant(None)
    };
    let report = run_conformance(fixture(
        MockAgent::broadcast(),
        vec![text, image, video],
        library.all(),
        Vec::new(),
    ))
    .await;
    assert_skipped(&report, &["oauth_config_fields", "direct"]);
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
    let report = run_conformance(fixture(
        MockAgent::direct(),
        vec![email],
        Vec::new(),
        recipients,
    ))
    .await;
    assert_skipped(&report, &["oauth_config_fields"]);
}

#[sqlx::test(migrations = "../../data/migrations")]
async fn mock_oauth_passes(pool: PgPool) {
    let library = library(&pool).await;
    let provider = url::Url::parse("https://provider.test/").unwrap();
    let agent = MockAgent::oauth(&provider);
    let declared = agent.descriptor().formats.len();
    assert_eq!(declared, 3);
    let image = Variant {
        target: account("mock_oauth"),
        media: items(&library.images),
        ..variant(None)
    };
    let video = Variant {
        target: account("mock_oauth"),
        media: items(std::slice::from_ref(&library.video)),
        ..variant(None)
    };
    let text = Variant {
        target: account("mock_oauth"),
        ..variant(None)
    };
    // No check uses the deployment config (`credentials` is skipped for an OAuth agent and
    // `oauth_config_fields` reads the schema), so the fixture's default context serves.
    let report = run_conformance(fixture(
        agent,
        vec![text, image, video],
        library.all(),
        Vec::new(),
    ))
    .await;
    assert_skipped(&report, &["credentials", "direct"]);
}
