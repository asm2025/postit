//! Test doubles for the drafter and the crates that use it: a scripted provider and the factory
//! that hands it out, frame extractors with no frames and with fake ones, and agent descriptors
//! with no agent crate behind them.
//!
//! ```ignore
//! let provider = Arc::new(
//!     ScriptedProvider::new()
//!         .text(r#"{"variants": []}"#)
//!         .fail(FailureReason::RateLimited),
//! );
//! let factory = Arc::new(ScriptedFactory::new(provider.clone()));
//! // ... build the service with `factory`, call it, then:
//! assert_eq!(provider.requests().len(), 2);
//! ```

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    Agent, AgentClient, AgentContext, AgentDescriptor, AgentError, AgentKind, AgentRegistry,
    Capability, ConnectMethod, ContextImage, FormatId, FormatSpec, FrameExtractor, MediaAsset,
    MediaError, MediaKind, MediaRules, OptionsSchema, PlatformId, TextMeasure, TextRules,
    VideoContext,
};
use secrecy::ExposeSecret as _;
use tokio::time::Instant;

use crate::config::{ProviderConfig, ProviderSecret};
use crate::error::{DrafterError, FailureReason, LlmError};
use crate::factory::ProviderFactory;
use crate::presets::Preset;
use crate::provider::{ChatRequest, ChatResponse, Finish, LlmProvider, RemoteModel, Usage};

/// The usage every [`ScriptedProvider::text`] answer reports.
pub const SCRIPTED_USAGE: Usage = Usage {
    input_tokens: 100,
    output_tokens: 20,
};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Scripted {
    answer: Result<ChatResponse, LlmError>,
    delay: Option<Duration>,
}

/// An [`LlmProvider`] that answers from a queue, in order, and records every request.
///
/// A chat with the queue empty answers `unavailable`. A delayed answer waits, but never past
/// the request's deadline: then it answers `timeout`, as a real provider would.
pub struct ScriptedProvider {
    answers: Mutex<VecDeque<Scripted>>,
    models: Mutex<Result<Vec<RemoteModel>, LlmError>>,
    requests: Mutex<Vec<ChatRequest>>,
    model_calls: AtomicUsize,
}

impl Default for ScriptedProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ScriptedProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedProvider")
            .field("queued", &self.remaining())
            .field("chat_calls", &self.chat_calls())
            .field("model_calls", &self.model_calls())
            .finish()
    }
}

impl ScriptedProvider {
    /// No answers queued, and an empty model list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            answers: Mutex::new(VecDeque::new()),
            models: Mutex::new(Ok(Vec::new())),
            requests: Mutex::new(Vec::new()),
            model_calls: AtomicUsize::new(0),
        }
    }

    /// Queues an answer.
    #[must_use]
    pub fn answer(self, answer: Result<ChatResponse, LlmError>) -> Self {
        self.push(answer);
        self
    }

    /// Queues an answer that arrives after `delay`.
    #[must_use]
    pub fn answer_after(self, delay: Duration, answer: Result<ChatResponse, LlmError>) -> Self {
        self.push_after(delay, answer);
        self
    }

    /// Queues a finished answer with `text` and [`SCRIPTED_USAGE`].
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.push_text(text);
        self
    }

    /// Queues a failure.
    #[must_use]
    pub fn fail(self, reason: FailureReason) -> Self {
        self.push(Err(LlmError::new(reason)));
        self
    }

    /// Sets what every model listing answers.
    #[must_use]
    pub fn models(self, models: Result<Vec<RemoteModel>, LlmError>) -> Self {
        self.set_models(models);
        self
    }

    /// Queues an answer on a shared provider.
    pub fn push(&self, answer: Result<ChatResponse, LlmError>) {
        lock(&self.answers).push_back(Scripted {
            answer,
            delay: None,
        });
    }

    /// Queues an answer that arrives after `delay` on a shared provider.
    pub fn push_after(&self, delay: Duration, answer: Result<ChatResponse, LlmError>) {
        lock(&self.answers).push_back(Scripted {
            answer,
            delay: Some(delay),
        });
    }

    /// Queues a finished answer with `text` and [`SCRIPTED_USAGE`] on a shared provider.
    pub fn push_text(&self, text: impl Into<String>) {
        self.push(Ok(ChatResponse {
            text: text.into(),
            usage: Some(SCRIPTED_USAGE),
            finish: Finish::Stop,
        }));
    }

    /// Sets what every model listing answers on a shared provider.
    pub fn set_models(&self, models: Result<Vec<RemoteModel>, LlmError>) {
        *lock(&self.models) = models;
    }

    /// Every chat request so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<ChatRequest> {
        lock(&self.requests).clone()
    }

    /// The latest chat request.
    #[must_use]
    pub fn last_request(&self) -> Option<ChatRequest> {
        lock(&self.requests).last().cloned()
    }

    /// How many chats were sent.
    #[must_use]
    pub fn chat_calls(&self) -> usize {
        lock(&self.requests).len()
    }

    /// How many model listings were asked for.
    #[must_use]
    pub fn model_calls(&self) -> usize {
        self.model_calls.load(Ordering::SeqCst)
    }

    /// How many answers are still queued.
    #[must_use]
    pub fn remaining(&self) -> usize {
        lock(&self.answers).len()
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        lock(&self.requests).push(request.clone());
        let next = lock(&self.answers).pop_front();
        let Some(Scripted { answer, delay }) = next else {
            return Err(LlmError::new(FailureReason::Unavailable));
        };
        if let Some(delay) = delay {
            let ready = Instant::now() + delay;
            if ready > request.deadline {
                tokio::time::sleep_until(request.deadline).await;
                return Err(LlmError::new(FailureReason::Timeout));
            }
            tokio::time::sleep_until(ready).await;
        }
        answer
    }

    async fn models(&self, _deadline: Instant) -> Result<Vec<RemoteModel>, LlmError> {
        self.model_calls.fetch_add(1, Ordering::SeqCst);
        lock(&self.models).clone()
    }
}

/// What a [`ScriptedFactory`] was asked to build. Holds the opened secret so tests can check
/// it arrived; its `Debug` shows only whether there was one.
#[derive(Clone)]
pub struct BuildRecord {
    /// The provider's config.
    pub config: ProviderConfig,
    /// The preset key, when the row came from one.
    pub preset: Option<&'static str>,
    /// The opened API key.
    pub api_key: Option<String>,
    /// The opened header values by name.
    pub headers: BTreeMap<String, String>,
}

impl fmt::Debug for BuildRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildRecord")
            .field("preset", &self.preset)
            .field("has_api_key", &self.api_key.is_some())
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// A [`ProviderFactory`] that hands out one shared [`ScriptedProvider`] and records each build.
#[derive(Debug)]
pub struct ScriptedFactory {
    provider: Arc<ScriptedProvider>,
    builds: Mutex<Vec<BuildRecord>>,
}

impl ScriptedFactory {
    /// A factory handing out `provider` for every row.
    #[must_use]
    pub fn new(provider: Arc<ScriptedProvider>) -> Self {
        Self {
            provider,
            builds: Mutex::new(Vec::new()),
        }
    }

    /// The provider it hands out.
    #[must_use]
    pub fn provider(&self) -> &Arc<ScriptedProvider> {
        &self.provider
    }

    /// Every build so far, in order.
    #[must_use]
    pub fn builds(&self) -> Vec<BuildRecord> {
        lock(&self.builds).clone()
    }

    /// The latest build.
    #[must_use]
    pub fn last_build(&self) -> Option<BuildRecord> {
        lock(&self.builds).last().cloned()
    }
}

impl ProviderFactory for ScriptedFactory {
    fn build(
        &self,
        config: &ProviderConfig,
        secret: ProviderSecret,
        preset: Option<&'static Preset>,
    ) -> Result<Arc<dyn LlmProvider>, DrafterError> {
        lock(&self.builds).push(BuildRecord {
            config: config.clone(),
            preset: preset.map(|p| p.key),
            api_key: secret.api_key.map(|k| k.expose_secret().to_owned()),
            headers: secret
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.expose_secret().to_owned()))
                .collect(),
        });
        Ok(self.provider.clone())
    }
}

/// A [`FrameExtractor`] that cannot make stills or frames, as on a server with no image or
/// `ffmpeg` support.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFrames;

#[async_trait]
impl FrameExtractor for NoFrames {
    async fn still(&self, _asset: &MediaAsset) -> Result<ContextImage, MediaError> {
        Err(MediaError::Unsupported("no stills in tests".to_owned()))
    }

    async fn frames(&self, _asset: &MediaAsset, _count: u32) -> Result<VideoContext, MediaError> {
        Err(MediaError::Unsupported("no frames in tests".to_owned()))
    }
}

/// A [`FrameExtractor`] that makes a small fake JPEG for every still and `count` fake frames for
/// every video, with the asset's probed duration, and counts how often it was asked.
#[derive(Debug, Default)]
pub struct FakeFrames {
    stills: AtomicUsize,
    videos: AtomicUsize,
}

impl FakeFrames {
    /// A fresh extractor with no calls counted.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many stills were asked for.
    #[must_use]
    pub fn stills(&self) -> usize {
        self.stills.load(Ordering::SeqCst)
    }

    /// How many videos were asked for frames.
    #[must_use]
    pub fn videos(&self) -> usize {
        self.videos.load(Ordering::SeqCst)
    }
}

fn fake_jpeg(label: String) -> ContextImage {
    ContextImage {
        jpeg: bytes::Bytes::from(label.into_bytes()),
        width: 16,
        height: 9,
    }
}

#[async_trait]
impl FrameExtractor for FakeFrames {
    async fn still(&self, asset: &MediaAsset) -> Result<ContextImage, MediaError> {
        self.stills.fetch_add(1, Ordering::SeqCst);
        Ok(fake_jpeg(format!("still-{}", asset.id)))
    }

    async fn frames(&self, asset: &MediaAsset, count: u32) -> Result<VideoContext, MediaError> {
        self.videos.fetch_add(1, Ordering::SeqCst);
        Ok(VideoContext {
            frames: (0..count)
                .map(|n| fake_jpeg(format!("frame-{n}-{}", asset.id)))
                .collect(),
            duration_ms: asset.duration_ms,
        })
    }
}

static ICON: &[u8] = &[0x89, b'P', b'N', b'G'];

fn platform_id(platform: &str) -> PlatformId {
    PlatformId::new(platform)
        .unwrap_or_else(|e| unreachable!("testkit platform ID {platform:?}: {e}"))
}

fn format(id: FormatId, text: TextRules, media: MediaRules) -> FormatSpec {
    FormatSpec {
        id,
        text,
        media,
        selected_by: None,
    }
}

fn descriptor(
    platform: &str,
    display_name: &str,
    kind: AgentKind,
    formats: Vec<FormatSpec>,
    capabilities: BTreeSet<Capability>,
) -> AgentDescriptor {
    AgentDescriptor {
        platform: platform_id(platform),
        display_name: display_name.to_owned(),
        icon_png: ICON,
        brand_color: "#336699".to_owned(),
        kind,
        connect: ConnectMethod::Credentials(OptionsSchema::empty()),
        formats,
        capabilities,
        config_schema_version: 1,
        native_schedule: None,
        quota_buckets: Vec::new(),
        options_schema: OptionsSchema::empty(),
    }
}

fn display_name(platform: &str) -> String {
    let mut chars = platform.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A broadcast platform with one required text format of `max` units, counted by `measure`,
/// and no media.
///
/// # Panics
/// When `platform` is not a valid platform ID.
#[must_use]
pub fn text_descriptor(platform: &str, max: u32, measure: TextMeasure) -> AgentDescriptor {
    descriptor(
        platform,
        &display_name(platform),
        AgentKind::Broadcast,
        vec![format(
            FormatId::Text,
            TextRules {
                measure,
                max,
                required: true,
                includes_link_and_tags: false,
            },
            MediaRules::none(),
        )],
        BTreeSet::new(),
    )
}

/// Like [`text_descriptor`] in graphemes, plus an options schema with a required `title`, so
/// a variant with no options gets an `option_required` violation.
///
/// # Panics
/// When `platform` is not a valid platform ID.
#[must_use]
pub fn options_descriptor(platform: &str, max: u32) -> AgentDescriptor {
    let mut descriptor = text_descriptor(platform, max, TextMeasure::Graphemes);
    descriptor.options_schema = OptionsSchema::from_json_str(
        r#"{"type":"object","properties":{"title":{"type":"string",
            "x-postit-label":{"key":"testkit.title","en":"Title"}}},
            "required":["title"],"additionalProperties":false}"#,
    )
    .unwrap_or_else(|e| unreachable!("testkit options schema: {e}"));
    descriptor
}

/// The caption limit of [`composed_descriptor`]'s photo format: a quarter of the text limit, as
/// Telegram's 1,024 is of its 4,096.
#[must_use]
pub const fn caption_max(max: u32) -> u32 {
    if max < 4 { 1 } else { max / 4 }
}

/// A Telegram-like broadcast platform that publishes text, link, and tags as one string
/// (`includes_link_and_tags`), counted in UTF-16 units: a required text format of `max` units
/// and a photo format (one image) whose optional caption holds [`caption_max`] units.
///
/// # Panics
/// When `platform` is not a valid platform ID.
#[must_use]
pub fn composed_descriptor(platform: &str, max: u32) -> AgentDescriptor {
    let text = |max, required| TextRules {
        measure: TextMeasure::Utf16Units,
        max,
        required,
        includes_link_and_tags: true,
    };
    descriptor(
        platform,
        &display_name(platform),
        AgentKind::Broadcast,
        vec![
            format(FormatId::Text, text(max, true), MediaRules::none()),
            format(
                FormatId::Image,
                text(caption_max(max), false),
                MediaRules {
                    min_count: 1,
                    max_count: 1,
                    kinds: vec![MediaKind::Image],
                    ..MediaRules::none()
                },
            ),
        ],
        BTreeSet::new(),
    )
}

/// The text limit of [`email_descriptor`]'s format.
pub const EMAIL_TEXT_MAX: u32 = 20_000;

/// An email-like direct platform `email` with one `HtmlEmail` format.
#[must_use]
pub fn email_descriptor() -> AgentDescriptor {
    descriptor(
        "email",
        "Email",
        AgentKind::Direct,
        vec![format(
            FormatId::HtmlEmail,
            TextRules {
                measure: TextMeasure::Graphemes,
                max: EMAIL_TEXT_MAX,
                required: false,
                includes_link_and_tags: false,
            },
            MediaRules {
                kinds: vec![MediaKind::Image],
                ..MediaRules::none()
            },
        )],
        BTreeSet::from([Capability::DirectSender]),
    )
}

/// An agent that only carries a descriptor; it makes no clients.
struct DescriptorAgent {
    descriptor: AgentDescriptor,
}

#[async_trait]
impl Agent for DescriptorAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn create_client(&self, _ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Err(AgentError::Transport {
            message: "a testkit agent has no client".to_owned(),
        })
    }
}

/// A registry holding one descriptor-only agent per descriptor.
///
/// # Panics
/// When a descriptor is invalid or two share a platform.
#[must_use]
pub fn registry(descriptors: impl IntoIterator<Item = AgentDescriptor>) -> AgentRegistry {
    let mut registry = AgentRegistry::new();
    for descriptor in descriptors {
        let platform = descriptor.platform.clone();
        registry
            .register(Arc::new(DescriptorAgent { descriptor }))
            .unwrap_or_else(|e| unreachable!("testkit descriptor {platform}: {e}"));
    }
    registry
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_register() {
        let r = registry([
            text_descriptor("textish", 300, TextMeasure::Graphemes),
            composed_descriptor("telegramish", 4_096),
            email_descriptor(),
        ]);
        assert_eq!(r.len(), 3);
        let tg = r
            .get(&PlatformId::new("telegramish").unwrap())
            .unwrap()
            .descriptor();
        assert_eq!(tg.display_name, "Telegramish");
        assert_eq!(tg.formats[0].text.max, 4_096);
        assert_eq!(tg.formats[1].text.max, 1_024);
        assert!(tg.formats.iter().all(|f| f.text.includes_link_and_tags));
        let email = r
            .get(&PlatformId::new("email").unwrap())
            .unwrap()
            .descriptor();
        assert_eq!(email.kind, AgentKind::Direct);
        assert_eq!(email.formats[0].id, FormatId::HtmlEmail);
        assert_eq!(caption_max(2), 1);
    }

    #[tokio::test]
    async fn scripted_provider_answers_in_order_and_records() {
        let p = ScriptedProvider::new()
            .text("one")
            .fail(FailureReason::RateLimited);
        let req = ChatRequest {
            model: "m".to_owned(),
            system: String::new(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: 1,
            output: crate::provider::OutputMode::Text,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(p.chat(&req).await.unwrap().text, "one");
        assert_eq!(
            p.chat(&req).await.unwrap_err().reason,
            FailureReason::RateLimited
        );
        assert_eq!(
            p.chat(&req).await.unwrap_err().reason,
            FailureReason::Unavailable
        );
        assert_eq!(p.chat_calls(), 3);
        assert_eq!(p.requests()[0].model, "m");
    }

    #[tokio::test]
    async fn scripted_delay_never_outlives_the_deadline() {
        let answer = |text: &str| {
            Ok(ChatResponse {
                text: text.to_owned(),
                usage: None,
                finish: Finish::Stop,
            })
        };
        let p = ScriptedProvider::new()
            .answer_after(Duration::from_millis(20), answer("late"))
            .answer_after(Duration::from_secs(60), answer("too late"));
        let mut req = ChatRequest {
            model: "m".to_owned(),
            system: String::new(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: 1,
            output: crate::provider::OutputMode::Text,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(p.chat(&req).await.unwrap().text, "late");
        req.deadline = Instant::now() + Duration::from_millis(50);
        let started = Instant::now();
        assert_eq!(
            p.chat(&req).await.unwrap_err().reason,
            FailureReason::Timeout
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
