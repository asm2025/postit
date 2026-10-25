//! A wiremock platform for [`echo::EchoAgent`], the [`Faults`] that break it, and the
//! conformance fixture over both.

pub mod echo;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use postit_agent_kit::testkit::conformance::{
    ConformanceFixture, CredentialSamples, Fault, Faults,
};
use postit_agent_kit::testkit::{TestContext, responders};
use postit_core::{
    AccountId, AccountRef, CredentialFields, Credentials, DirectRecipient, DirectSendId,
    MediaAssetId, MediaItem, PlatformId, PostKind, Variant,
};
use secrecy::SecretString;
use serde_json::{Map, json};
use uuid::Uuid;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Match, Mock, MockServer, Request, Respond, ResponseTemplate};

use echo::{Defects, EchoAgent, PLATFORM};

/// The token the fixture connects with; the suite checks it never shows.
pub const TOKEN: &str = "echo-s3cret-token";
/// The account the token reaches.
pub const ACCOUNT: &str = "me-1";
/// The sample image's size: two full 64 KiB chunks and a short last one.
pub const IMAGE_BYTES: usize = 150 * 1024;

#[derive(Default)]
struct Platform {
    /// Post ID by idempotency key.
    posts: HashMap<String, String>,
    next_post: u32,
    next_upload: u32,
    /// The `ignores_key` defect: every create makes a new post.
    ignores_key: bool,
}

impl Platform {
    /// The post for `key`, created on first sight (and every time when `ignores_key`).
    fn post_for(&mut self, key: &str) -> String {
        if let Some(id) = self.posts.get(key).filter(|_| !self.ignores_key) {
            return id.clone();
        }
        self.next_post += 1;
        let id = format!("post-{}", self.next_post);
        self.posts.insert(key.to_owned(), id.clone());
        id
    }
}

type Shared = Arc<Mutex<Platform>>;

#[derive(Default)]
struct Counters {
    remote: AtomicUsize,
    mutating: AtomicUsize,
}

/// Counts every request that reaches the platform, then answers with `inner`.
struct Counted<R> {
    /// `None` leaves the request uncounted (the `uncounted_faults` fixture defect).
    counters: Option<Arc<Counters>>,
    inner: R,
}

impl<R: Respond> Respond for Counted<R> {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if let Some(counters) = &self.counters {
            counters.remote.fetch_add(1, Ordering::SeqCst);
            if request.method != wiremock::http::Method::GET {
                counters.mutating.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.inner.respond(request)
    }
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn authorized(request: &Request) -> bool {
    header(request, "authorization").as_deref() == Some(&format!("Bearer {TOKEN}"))
}

fn created(id: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "id": id }))
}

/// The echo platform on a wiremock server, with fault injection.
///
/// Faults are served by one mock mounted at a higher priority than the happy path, which
/// matches only while a fault is armed. (Scoped wiremock mocks cannot be used for this:
/// wiremock 0.6 re-sorts its mocks by priority on each request, so dropping a scoped mock's
/// guard can unmount a different mock.)
pub struct EchoFaults {
    server: Arc<MockServer>,
    platform: Shared,
    counters: Arc<Counters>,
    armed: Armed,
    defects: Defects,
}

/// The armed fault, shared with the fault mock.
type Armed = Arc<Mutex<Option<Fault>>>;

/// Matches while a fault is armed.
struct WhileArmed(Armed);

impl Match for WhileArmed {
    fn matches(&self, _: &Request) -> bool {
        self.0.lock().unwrap().is_some()
    }
}

impl EchoFaults {
    async fn start(defects: Defects) -> Self {
        let server = Arc::new(MockServer::start().await);
        let faults = Self {
            server,
            platform: Arc::new(Mutex::new(Platform {
                ignores_key: defects.ignores_key,
                ..Platform::default()
            })),
            counters: Arc::default(),
            armed: Armed::default(),
            defects,
        };
        faults.mount_happy_path().await;
        faults.mount_fault().await;
        faults
    }

    fn counted<R: Respond>(&self, inner: R) -> Counted<R> {
        Counted {
            counters: Some(self.counters.clone()),
            inner,
        }
    }

    async fn mount_happy_path(&self) {
        let server = &*self.server;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(self.counted(|request: &Request| {
                if authorized(request) {
                    ResponseTemplate::new(200).set_body_json(
                        json!({"id": ACCOUNT, "name": "Echo Shop", "handle": "@shop"}),
                    )
                } else {
                    ResponseTemplate::new(401)
                }
            }))
            .mount(server)
            .await;

        let platform = self.platform.clone();
        Mock::given(method("POST"))
            .and(path("/uploads"))
            .respond_with(self.counted(move |_: &Request| {
                let mut platform = platform.lock().unwrap();
                platform.next_upload += 1;
                created(&format!("up-{}", platform.next_upload), 201)
            }))
            .mount(server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(r"^/uploads/[^/]+$"))
            .respond_with(self.counted(|request: &Request| {
                match header(request, "content-range") {
                    Some(_) => ResponseTemplate::new(204),
                    None => ResponseTemplate::new(400),
                }
            }))
            .mount(server)
            .await;

        let platform = self.platform.clone();
        Mock::given(method("POST"))
            .and(path("/posts"))
            .respond_with(self.counted(move |request: &Request| {
                let Some(key) = header(request, "idempotency-key") else {
                    return ResponseTemplate::new(400);
                };
                let mut platform = platform.lock().unwrap();
                let fresh = !platform.posts.contains_key(&key);
                created(&platform.post_for(&key), if fresh { 201 } else { 200 })
            }))
            .mount(server)
            .await;

        let platform = self.platform.clone();
        Mock::given(method("GET"))
            .and(path("/posts"))
            .respond_with(self.counted(move |request: &Request| {
                let key = request
                    .url
                    .query_pairs()
                    .find(|(k, _)| k == "key")
                    .map(|(_, v)| v.into_owned());
                let platform = platform.lock().unwrap();
                match key.and_then(|k| platform.posts.get(&k).cloned()) {
                    Some(id) => created(&id, 200),
                    None => ResponseTemplate::new(404),
                }
            }))
            .mount(server)
            .await;
    }
}

impl EchoFaults {
    /// The create call fails as the armed fault says.
    async fn mount_fault(&self) {
        let armed = self.armed.clone();
        let platform = self.platform.clone();
        let mut responder = self.counted(move |request: &Request| {
            let Some(fault) = *armed.lock().unwrap() else {
                return ResponseTemplate::new(500);
            };
            match fault {
                Fault::Unauthorized => ResponseTemplate::new(401),
                Fault::RateLimited(after) => responders::rate_limited(after.map(|d| d.as_secs())),
                Fault::ServerError => responders::server_error(),
                Fault::LostBeforeAccept => responders::dropped(),
                Fault::Malformed => responders::malformed_json(),
                Fault::AcceptedThenLost => {
                    if let Some(key) = header(request, "idempotency-key") {
                        platform.lock().unwrap().post_for(&key);
                    }
                    responders::dropped()
                }
            }
        });
        if self.defects.uncounted_faults {
            responder.counters = None;
        }
        Mock::given(method("POST"))
            .and(path("/posts"))
            .and(WhileArmed(self.armed.clone()))
            .respond_with(responder)
            .with_priority(1)
            .mount(&self.server)
            .await;
    }
}

#[async_trait]
impl Faults for EchoFaults {
    async fn arm(&self, fault: Fault) {
        *self.armed.lock().unwrap() = Some(fault);
    }

    async fn clear(&self) {
        *self.armed.lock().unwrap() = None;
    }

    fn remote_calls(&self) -> usize {
        if self.defects.frozen_counters {
            return 0;
        }
        self.counters.remote.load(Ordering::SeqCst)
    }

    fn mutating_calls(&self) -> usize {
        if self.defects.frozen_counters {
            return 0;
        }
        self.counters.mutating.load(Ordering::SeqCst)
    }
}

fn token_fields(token: &str) -> CredentialFields {
    CredentialFields::from_parts(
        Map::new(),
        BTreeMap::from([("token".to_owned(), SecretString::from(token.to_owned()))]),
    )
}

fn recipient(name: &str) -> DirectRecipient {
    DirectRecipient {
        send_id: DirectSendId::from_uuid(Uuid::now_v7()),
        address: format!("{name}@example.com"),
        merge_fields: BTreeMap::new(),
        locale: "en".to_owned(),
    }
}

fn variant(account: &AccountRef, text: &str, media: Vec<MediaItem>) -> Variant {
    Variant {
        target: account.clone(),
        kind: PostKind::Post,
        text: text.to_owned(),
        media,
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: Map::new(),
    }
}

/// A conformance fixture for an [`EchoAgent`] with `defects`, on a fresh wiremock server.
pub async fn echo_fixture(defects: Defects) -> ConformanceFixture {
    let faults = Arc::new(EchoFaults::start(defects).await);
    let config = json!({ "base_url": faults.server.uri() });
    let test =
        Arc::new(TestContext::new().with_credentials(Credentials::Fields(token_fields(TOKEN))));
    let image = MediaAssetId::from_uuid(Uuid::now_v7());
    let pixels: Vec<u8> = (0..IMAGE_BYTES)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    test.media_store.insert(image, Bytes::from(pixels));
    let account = AccountRef {
        platform: PlatformId::new(PLATFORM).unwrap(),
        account: AccountId::from_uuid(Uuid::now_v7()),
        remote_id: ACCOUNT.to_owned(),
    };
    let mut samples = vec![
        variant(
            &account,
            "A picture",
            vec![MediaItem {
                asset: image,
                alt_text: Some("A test pattern".to_owned()),
            }],
        ),
        variant(&account, "Hello from echo", Vec::new()),
    ];
    if defects.uncounted_faults {
        samples.reverse();
    }
    let context = {
        let test = test.clone();
        let config = config.clone();
        Arc::new(move || test.agent_context(config.clone()))
    };
    ConformanceFixture {
        agent: Arc::new(EchoAgent::new(defects)),
        context,
        check_context: test.check_context(config),
        account,
        samples,
        credentials: Some(CredentialSamples {
            valid: token_fields(TOKEN),
            invalid: token_fields("wrong-token"),
        }),
        recipients: if defects.drops_recipient {
            ["ana", "ben"].map(recipient).into()
        } else {
            Vec::new()
        },
        deduplicates: true,
        secrets: vec![TOKEN.to_owned()],
        faults,
    }
}
