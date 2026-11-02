//! The conformance suite against the Telegram agent on a fake Bot API.
#![allow(clippy::unwrap_used, clippy::panic, clippy::needless_pass_by_value)]

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use common::{BOT_ID, TOKEN, account, fields};
use postit_agent_kit::testkit::conformance::{
    ConformanceFixture, CredentialSamples, Fault, Faults, run_conformance,
};
use postit_agent_kit::testkit::{TestContext, responders};
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_core::{
    AccountInfo, AccountRef, Agent, AgentClient, AgentContext, AgentDescriptor, AgentError,
    CredentialCheckContext, CredentialFields, Credentials, Discovered, MediaAsset, MediaAssetId,
    MediaAssetState, MediaItem, MediaKind, OptionsSchema, PostKind, PublishOutcome,
    PublishProgress, PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest, UserId,
    Variant, Violation,
};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Match, Mock, MockServer, Request, Respond, ResponseTemplate};

const CHAT: &str = "-1001234567890";

#[derive(Default)]
struct Counters {
    remote: AtomicUsize,
    mutating: AtomicUsize,
}

type Armed = Arc<Mutex<Option<Fault>>>;

struct WhileArmed(Armed);

impl Match for WhileArmed {
    fn matches(&self, _: &Request) -> bool {
        self.0.lock().unwrap().is_some()
    }
}

/// Counts every request, and the sends among them, then answers with `inner`.
struct Counted<R> {
    counters: Arc<Counters>,
    sends: bool,
    inner: R,
}

impl<R: Respond> Respond for Counted<R> {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self.counters.remote.fetch_add(1, Ordering::SeqCst);
        if self.sends {
            self.counters.mutating.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.respond(request)
    }
}

struct BotFaults {
    _server: MockServer,
    counters: Arc<Counters>,
    armed: Armed,
}

fn result(value: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": value}))
}

fn sent() -> Value {
    json!({
        "message_id": 5,
        "chat": {"id": -1_001_234_567_890_i64, "type": "channel", "username": "conformance"},
        "date": 1
    })
}

fn token_of(request: &Request) -> String {
    request
        .url
        .path()
        .trim_start_matches("/bot")
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

impl BotFaults {
    async fn start() -> (Self, String) {
        let server = MockServer::start().await;
        let counters = Arc::new(Counters::default());
        let armed: Armed = Arc::default();
        let counted =
            |sends: bool, inner: Box<dyn Fn(&Request) -> ResponseTemplate + Send + Sync>| Counted {
                counters: Arc::clone(&counters),
                sends,
                inner: move |request: &Request| inner(request),
            };

        Mock::given(method("POST"))
            .and(path_regex(r"^/bot[^/]+/getMe$"))
            .respond_with(counted(
                false,
                Box::new(|request| {
                    if token_of(request) == TOKEN {
                        result(json!({"id": BOT_ID, "is_bot": true, "username": "postit_bot"}))
                    } else {
                        ResponseTemplate::new(401).set_body_json(
                            json!({"ok": false, "error_code": 401, "description": "Unauthorized"}),
                        )
                    }
                }),
            ))
            .mount(&server)
            .await;
        for (name, body) in [
            ("getUpdates", json!([])),
            (
                "getChat",
                json!({"id": -1_001_234_567_890_i64, "type": "channel", "title": "Conformance", "username": "conformance"}),
            ),
            (
                "getChatMember",
                json!({"status": "administrator", "can_post_messages": true}),
            ),
        ] {
            Mock::given(method("POST"))
                .and(path_regex(format!("^/bot[^/]+/{name}$")))
                .respond_with(counted(false, Box::new(move |_| result(body.clone()))))
                .mount(&server)
                .await;
        }
        for (pattern, body) in [
            (
                r"^/bot[^/]+/send(Message|Photo|Video|Animation|Document)$",
                sent(),
            ),
            (r"^/bot[^/]+/sendMediaGroup$", json!([sent(), sent()])),
        ] {
            Mock::given(method("POST"))
                .and(path_regex(pattern))
                .respond_with(counted(true, Box::new(move |_| result(body.clone()))))
                .mount(&server)
                .await;
        }
        // One higher-priority mock serves the armed fault on every send.
        let fault_armed = Arc::clone(&armed);
        Mock::given(method("POST"))
            .and(path_regex(r"^/bot[^/]+/send[A-Za-z]+$"))
            .and(WhileArmed(Arc::clone(&armed)))
            .respond_with(counted(
                true,
                Box::new(move |_| match (*fault_armed.lock().unwrap()).unwrap() {
                    Fault::Unauthorized => ResponseTemplate::new(401).set_body_json(
                        json!({"ok": false, "error_code": 401, "description": "Unauthorized"}),
                    ),
                    Fault::RateLimited(after) => {
                        responders::rate_limited(after.map(|d| d.as_secs()))
                    }
                    Fault::ServerError => responders::server_error(),
                    Fault::Malformed => responders::malformed_json(),
                    Fault::AcceptedThenLost | Fault::LostBeforeAccept => responders::dropped(),
                }),
            ))
            .with_priority(1)
            .mount(&server)
            .await;

        let uri = server.uri();
        (
            Self {
                _server: server,
                counters,
                armed,
            },
            uri,
        )
    }
}

#[async_trait]
impl Faults for BotFaults {
    async fn arm(&self, fault: Fault) {
        *self.armed.lock().unwrap() = Some(fault);
    }

    async fn clear(&self) {
        *self.armed.lock().unwrap() = None;
    }

    fn remote_calls(&self) -> usize {
        self.counters.remote.load(Ordering::SeqCst)
    }

    fn mutating_calls(&self) -> usize {
        self.counters.mutating.load(Ordering::SeqCst)
    }
}

fn asset(kind: MediaKind, mime: &str, dims: (u32, u32)) -> MediaAsset {
    MediaAsset {
        id: MediaAssetId::from_uuid(Uuid::now_v7()),
        owner_id: UserId::from_uuid(Uuid::now_v7()),
        kind,
        mime: mime.to_owned(),
        bytes: 2_048,
        width: Some(dims.0),
        height: Some(dims.1),
        duration_ms: (kind == MediaKind::Video).then_some(3_000),
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

fn sample(text: &str, assets: &[&MediaAsset]) -> Variant {
    Variant {
        target: account(CHAT),
        kind: PostKind::Post,
        text: text.to_owned(),
        media: assets
            .iter()
            .map(|a| MediaItem {
                asset: a.id,
                alt_text: None,
            })
            .collect(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: Map::new(),
    }
}

fn wrong_token() -> CredentialFields {
    let secret = BTreeMap::from([("bot_token".to_owned(), SecretString::from("999:wrong"))]);
    CredentialFields::from_parts(Map::new(), secret)
}

async fn fixture(wrap: impl FnOnce(TelegramAgent) -> Arc<dyn Agent>) -> ConformanceFixture {
    let (faults, uri) = BotFaults::start().await;
    let test = Arc::new(
        TestContext::new().with_credentials(Credentials::Fields(fields(Some("@conformance")))),
    );
    let image = asset(MediaKind::Image, "image/jpeg", (64, 64));
    let first = asset(MediaKind::Image, "image/png", (64, 64));
    let second = asset(MediaKind::Image, "image/jpeg", (64, 64));
    let video = asset(MediaKind::Video, "video/mp4", (640, 360));
    for a in [&image, &first, &second, &video] {
        test.media_store
            .insert_owned(a.owner_id, a.id, Bytes::from(vec![9_u8; 2_048]));
    }
    let samples = vec![
        sample("A picture", &[&image]),
        sample("Hello", &[]),
        sample("An album", &[&first, &second]),
        sample("A clip", &[&video]),
    ];
    let agent = TelegramAgent::new(TelegramConfig {
        api_base: Url::parse(&uri).unwrap(),
        upload_timeout: Duration::from_secs(1),
    });
    let context: Arc<dyn Fn() -> AgentContext + Send + Sync> = {
        let test = Arc::clone(&test);
        Arc::new(move || test.agent_context(Value::Null))
    };
    ConformanceFixture {
        agent: wrap(agent),
        context,
        check_context: test.check_context(Value::Null),
        account: account(CHAT),
        samples,
        assets: vec![image, first, second, video],
        credentials: Some(CredentialSamples {
            valid: fields(Some("@conformance")),
            invalid: wrong_token(),
        }),
        recipients: Vec::new(),
        secrets: vec![TOKEN.to_owned()],
        faults: Arc::new(faults),
        deduplicates: false,
    }
}

#[tokio::test]
async fn telegram_passes_conformance() {
    let report = run_conformance(fixture(|agent| Arc::new(agent)).await).await;
    report.assert_passed();
    assert_eq!(
        report.skipped,
        ["idempotency", "oauth_config_fields", "direct"]
    );
}

/// An agent that puts the token in its publish errors, as a careless mapping would.
struct Leaky(TelegramAgent);

struct LeakyClient(Box<dyn AgentClient>);

#[async_trait]
impl Agent for Leaky {
    fn descriptor(&self) -> &AgentDescriptor {
        self.0.descriptor()
    }

    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(LeakyClient(self.0.create_client(ctx)?)))
    }

    async fn verify_credentials(
        &self,
        ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        self.0.verify_credentials(ctx, fields).await
    }
}

#[async_trait]
impl AgentClient for LeakyClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        self.0.account_info(account).await
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        self.0.discover_accounts().await
    }

    async fn publish_options(&self, account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        self.0.publish_options(account).await
    }

    async fn validate(
        &self,
        variant: &Variant,
        media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        self.0.validate(variant, media).await
    }

    async fn publish(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        self.0
            .publish(request, progress)
            .await
            .map_err(|err| match err {
                AgentError::Remote { status, code, .. } => AgentError::Remote {
                    status,
                    code,
                    message: format!("failed with bot{TOKEN}"),
                },
                AgentError::Authentication { .. } => AgentError::Authentication {
                    message: format!("bot{TOKEN} was refused"),
                },
                other => other,
            })
    }

    async fn status(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        self.0.status(account, receipt).await
    }

    async fn reconcile(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        self.0.reconcile(request).await
    }
}

#[tokio::test]
async fn conformance_catches_a_token_leak() {
    let report = run_conformance(fixture(|agent| Arc::new(Leaky(agent))).await).await;
    assert!(
        report.failures.iter().any(|f| f.check == "redaction"),
        "{:#?}",
        report.failures
    );
}
