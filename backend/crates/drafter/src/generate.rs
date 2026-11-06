//! Generation: [`DrafterService::generate`] in every mode, validated against each target's
//! descriptor with one repair, plus the hooks E1 and D3 call and the owner's history.
//!
//! The order inside `generate` is fixed: the request's own limits, the targets through the
//! scope, the media, the comment (reply mode), the provider and model, the target briefs, the
//! media context, then the provider call, parsing, validation, at most one repair, and the
//! generation row. A request refused before the provider is called spends nothing and writes
//! no row.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, AccountRef, Agent, AiGenerationId, AiProviderId, ContextImage, DeliveryId,
    EmailDocument, EmailLanguage, FormatId, InboxItemId, MediaAsset, MediaAssetId, MediaItem,
    MediaKind, Message, PostId, PostKind, ReplyTo, Schedule, TextMeasure, TextRules,
    ValidationInput, Variant, VideoContext, Violation, ViolationCode, pick_format, resolve_variant,
    validate_target,
};
use postit_data::accounts::{Account, AccountRepo};
use postit_data::ai_generations::{
    AiGeneration, AiGenerationRepo, GenerationOutcome, NewAiGeneration, prompt_hash,
};
use postit_data::brand_profiles::BrandProfileRepo;
use postit_data::inbox_items::InboxItemRepo;
use postit_data::media::{MediaRepo, MediaStatus};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::publishing::{DeliveryRepo, JobRepo};
use postit_data::writing_styles::{WritingStyleFields, WritingStyleRepo};
use postit_data::{Capability, DataError, OwnerScope};
use secrecy::ExposeSecret as _;
use serde_json::{Map, Value};
use sqlx::PgConnection;
use tokio::time::Instant;
use url::Url;

use crate::error::{DrafterError, FailureReason, LlmError};
use crate::output::{Draft, Parsed, parse, to_draft};
use crate::presets::StructuredOutput;
use crate::prompt::{
    Mode, PromptInput, ReplyContext, TargetBrief, language_for, measure_unit, output_schema,
    repair_message, system_prompt, user_message,
};
use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, Finish, OutputMode, Part, Role, Usage,
};
use crate::service::{DrafterService, Loaded, check_model, data, require, violation};

/// The longest prompt, in characters. In rewrite mode the prompt is the text to rewrite, so the
/// same limit applies to it, through the route and [`DrafterService::rewrite`] alike: a longer
/// post is refused with `ai.prompt_too_long` rather than sent at the user's cost.
pub const PROMPT_MAX: usize = 8_000;

/// The most targets one request drafts for.
pub const TARGETS_MAX: usize = 20;

/// The most media one request carries.
pub const MEDIA_MAX: usize = 10;

/// The most images one request shows the model.
pub const IMAGES_MAX: usize = 4;

/// The frames one video shows the model.
pub const FRAMES_PER_VIDEO: u32 = postit_core::CONTEXT_MAX_FRAMES;

/// The most pictures one request shows the model in all: image stills and video frames.
pub const PICTURES_MAX: usize = 8;

/// Preparing media may take at most this fraction (one over it) of `ai.request_timeout`.
const MEDIA_SHARE: u32 = 4;

/// The name the answer's JSON schema is sent under.
const SCHEMA_NAME: &str = "drafts";

/// The interface language when the actor has no preferences row.
const DEFAULT_LOCALE: &str = "en-US";

const CUT_OFF: &str = "Your answer was cut off at the length limit. Write shorter variants so \
                       the whole JSON object fits.";
const REFUSED: &str = "Your answer was declined. Write the drafts as asked, within the rules.";
const UNREADABLE: &str = "Your answer was empty or could not be read. Answer with the JSON \
                          object asked for.";
const NOT_JSON: &str = "Your answer was not one JSON object in the shape asked for.";
const NO_TARGET: &str = "Your answer had no variant for any target. Give one variant for every \
                         target, with the target IDs exactly as given.";

/// A generation request. `Debug` shows the mode, sizes, and IDs, never the prompt.
#[derive(Clone)]
pub struct GenerateRequest {
    /// What to write.
    pub mode: Mode,
    /// The user's prompt, or the text to rewrite.
    pub prompt: String,
    /// The accounts to write for.
    pub targets: Vec<AccountId>,
    /// The language to write in.
    pub language: Option<String>,
    /// Media the post carries.
    pub media_ids: Vec<MediaAssetId>,
    /// The inbox item to answer, in reply mode.
    pub comment_id: Option<InboxItemId>,
    /// The provider; the owner's default when `None`.
    pub provider_id: Option<AiProviderId>,
    /// The model; the provider's default when `None`.
    pub model: Option<String>,
    /// The link the post carries.
    pub link: Option<Url>,
}

impl fmt::Debug for GenerateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerateRequest")
            .field("mode", &self.mode)
            .field("prompt_chars", &self.prompt.chars().count())
            .field("targets", &self.targets)
            .field("has_language", &self.language.is_some())
            .field("media_ids", &self.media_ids.len())
            .field("comment_id", &self.comment_id)
            .field("provider_id", &self.provider_id)
            .field("model", &self.model)
            .field("has_link", &self.link.is_some())
            .finish()
    }
}

/// A generation's result. `Debug` shows IDs and counts, never the drafted text.
#[derive(Clone)]
pub struct Generation {
    /// The generation row.
    pub id: AiGenerationId,
    /// The provider used.
    pub provider_id: AiProviderId,
    /// The model used.
    pub model: String,
    /// The draft.
    pub draft: Draft,
    /// What is still wrong, per target; targets with nothing wrong are left out.
    pub violations: BTreeMap<AccountId, Vec<Violation>>,
    /// Notes for the user.
    pub notes: Vec<Message>,
    /// Tokens spent, summed over every call.
    pub usage: Option<Usage>,
}

impl fmt::Debug for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let violations: BTreeMap<&AccountId, usize> =
            self.violations.iter().map(|(a, v)| (a, v.len())).collect();
        let notes: Vec<&str> = self.notes.iter().map(|n| n.key.as_ref()).collect();
        f.debug_struct("Generation")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("model", &self.model)
            .field("draft", &self.draft)
            .field("violations", &violations)
            .field("notes", &notes)
            .field("usage", &self.usage)
            .finish()
    }
}

/// One target account with its platform's agent.
struct Target {
    account: Account,
    agent: Arc<dyn Agent>,
}

impl Target {
    fn account_ref(&self) -> AccountRef {
        AccountRef {
            platform: self.account.platform.clone(),
            account: self.account.id,
            remote_id: self.account.remote_account_id.clone(),
        }
    }

    fn is_email(&self) -> bool {
        self.agent
            .descriptor()
            .formats
            .iter()
            .any(|f| f.id == FormatId::HtmlEmail)
    }
}

/// The comment a reply answers and where the reply goes.
struct Reply {
    context: ReplyContext,
    to: ReplyTo,
}

/// What every answer is checked against.
struct Plan {
    mode: Mode,
    targets: Vec<Target>,
    briefs: Vec<TargetBrief>,
    assets: Vec<MediaAsset>,
    items: Vec<MediaItem>,
    link: Option<Url>,
    reply_to: Option<ReplyTo>,
}

impl Plan {
    /// The campaign language of an email target: its writing language when postit renders
    /// campaigns in it, else English.
    fn email_language(&self, account: AccountId) -> EmailLanguage {
        self.briefs
            .iter()
            .find(|b| b.account == account)
            .and_then(|b| b.language.as_deref())
            .and_then(EmailLanguage::for_tag)
            .unwrap_or(EmailLanguage::EnUs)
    }
}

/// An answer checked: the variants and the draft with what is still wrong, when it parsed
/// with at least one target answered, and what a repair must fix.
struct Evaluated {
    parsed: Option<Parsed>,
    checked: Option<(Draft, BTreeMap<AccountId, Vec<Violation>>)>,
    problems: Vec<String>,
}

/// What the model is shown of the media.
#[derive(Default)]
struct MediaContext {
    images: Vec<Part>,
    note: Option<String>,
    notes: Vec<Message>,
}

/// What the generation row records besides its outcome and usage.
struct Record<'a> {
    provider_id: AiProviderId,
    model: &'a str,
    mode: Mode,
    prompt: &'a str,
    store_prompt: bool,
}

fn message(key: &'static str, en: impl Into<Cow<'static, str>>) -> Message {
    Message {
        key: Cow::Borrowed(key),
        en: en.into(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// What a text-only model is told of the media: that it is there and unseen.
fn ignored_media(assets: &[MediaAsset]) -> MediaContext {
    let images = assets.iter().filter(|a| a.kind == MediaKind::Image).count();
    let videos = assets.len() - images;
    let carried = match (images, videos) {
        (i, 0) => plural(i, "image", "images"),
        (0, v) => plural(v, "video", "videos"),
        (i, v) => format!(
            "{} and {}",
            plural(i, "image", "images"),
            plural(v, "video", "videos")
        ),
    };
    MediaContext {
        images: Vec::new(),
        note: Some(format!(
            "The post carries {carried}, which you cannot see. Do not describe them."
        )),
        notes: vec![message(
            "ai.media_ignored",
            "This model reads text only, so your media was not shown to it.",
        )],
    }
}

/// A file to prepare for the model: an image's still, or a share of frames from a video.
enum Job<'a> {
    Still(&'a MediaAsset),
    Video(&'a MediaAsset, u32),
}

/// A [`Job`] done: `None` when the file could not be prepared in time.
enum Ready<'a> {
    Still(Option<ContextImage>),
    Video(&'a MediaAsset, u32, Option<VideoContext>),
}

/// Shares out the pictures in the post's order: [`FRAMES_PER_VIDEO`] per video and one per
/// image, at most [`IMAGES_MAX`] images and [`PICTURES_MAX`] pictures in all. Returns the jobs
/// and how many images were left out.
fn plan_media(assets: &[MediaAsset]) -> (Vec<Job<'_>>, usize) {
    let mut room = PICTURES_MAX;
    let (mut stills, mut left_out) = (0_usize, 0_usize);
    let mut jobs = Vec::new();
    for asset in assets {
        if asset.kind == MediaKind::Video {
            let wanted = FRAMES_PER_VIDEO.min(u32::try_from(room).unwrap_or(u32::MAX));
            room -= wanted as usize;
            jobs.push(Job::Video(asset, wanted));
        } else if stills < IMAGES_MAX && room > 0 {
            stills += 1;
            room -= 1;
            jobs.push(Job::Still(asset));
        } else {
            left_out += 1;
        }
    }
    (jobs, left_out)
}

/// The note line for a video and the frames of it the model is shown.
fn video_shown(asset: &MediaAsset, wanted: u32, cut: Option<VideoContext>) -> (String, Vec<Part>) {
    let duration = cut
        .as_ref()
        .and_then(|c| c.duration_ms)
        .or(asset.duration_ms);
    let length = duration.map_or_else(String::new, |ms| {
        format!(" of about {} seconds", ms.saturating_add(500) / 1000)
    });
    let shown: Vec<Part> = cut
        .map(|c| c.frames)
        .unwrap_or_default()
        .into_iter()
        .take(wanted as usize)
        .map(|f| Part::Image { jpeg: f.jpeg })
        .collect();
    let line = if shown.is_empty() {
        format!("A video{length} is on the post; it is not shown.")
    } else {
        format!(
            "A video{length} is on the post; {} from it are attached in time order.",
            plural(shown.len(), "frame", "frames")
        )
    };
    (line, shown)
}

/// Step 1: the request's own limits, checked before anything is read.
///
/// A blank prompt is refused in post mode even when media is attached: the roadmap's random
/// mode is a prompt plus optional media, and the media alone does not say what to write (a
/// text-only model does not even see it). Reply mode needs none, the comment is the subject.
fn check_request(request: &GenerateRequest) -> Result<(), DrafterError> {
    let mut v = Vec::new();
    if request.prompt.chars().count() > PROMPT_MAX {
        let en = if request.mode == Mode::Rewrite {
            "Keep the text to rewrite to 8,000 characters or fewer."
        } else {
            "Keep the request to 8,000 characters or fewer."
        };
        v.push(violation("prompt", "ai.prompt_too_long", en).param("max", PROMPT_MAX));
    }
    if request.prompt.contains('\0') {
        v.push(violation(
            "prompt",
            "ai.text_invalid",
            "Remove the hidden NUL characters from the request.",
        ));
    }
    if request.mode != Mode::Reply && request.prompt.trim().is_empty() {
        v.push(violation(
            "prompt",
            "ai.prompt_required",
            "Say what to write, or give the text to rewrite.",
        ));
    }
    let count = request.targets.len();
    if count == 0 || count > TARGETS_MAX {
        v.push(
            violation("targets", "ai.targets_count", "Choose 1 to 20 accounts.")
                .param("max", TARGETS_MAX),
        );
    }
    // Over the count, the count is the error; within it, the quadratic scan is at most 20 long.
    if count <= TARGETS_MAX {
        for (i, target) in request.targets.iter().enumerate() {
            if request.targets[..i].contains(target) {
                v.push(violation(
                    format!("targets[{i}]"),
                    "ai.target_duplicate",
                    "This account is chosen twice.",
                ));
            }
        }
    }
    if request.media_ids.len() > MEDIA_MAX {
        v.push(
            violation(
                "media_ids",
                "ai.media_too_many",
                "Attach at most 10 media files.",
            )
            .param("max", MEDIA_MAX),
        );
    } else {
        for (i, id) in request.media_ids.iter().enumerate() {
            if request.media_ids[..i].contains(id) {
                v.push(violation(
                    format!("media_ids[{i}]"),
                    "ai.media_duplicate",
                    "This media is attached twice.",
                ));
            }
        }
    }
    if request.mode == Mode::Reply {
        if request.comment_id.is_none() {
            v.push(violation(
                "comment_id",
                "ai.comment_required",
                "Choose the comment to answer.",
            ));
        }
        if count != 1 {
            v.push(violation(
                "targets",
                "ai.reply_one_target",
                "A reply goes to exactly one account: the one the comment is on.",
            ));
        }
    } else if request.comment_id.is_some() {
        v.push(violation(
            "comment_id",
            "ai.comment_unexpected",
            "Only a reply answers a comment.",
        ));
    }
    if let Some(model) = &request.model {
        check_model(model, "model", &mut v);
    }
    if v.is_empty() {
        Ok(())
    } else {
        Err(DrafterError::Validation(v))
    }
}

/// The answer shape: the config's `structured_output`, else the preset's, else `json_object`.
fn output_mode(loaded: &Loaded, briefs: &[TargetBrief]) -> OutputMode {
    let mode = match loaded.config.structured_output.as_deref() {
        Some("json_schema") => StructuredOutput::JsonSchema,
        Some("json_object") => StructuredOutput::JsonObject,
        Some("text") => StructuredOutput::Text,
        _ => loaded
            .preset
            .map_or(StructuredOutput::JsonObject, |p| p.structured_output),
    };
    match mode {
        StructuredOutput::JsonSchema => OutputMode::JsonSchema {
            name: SCHEMA_NAME.to_owned(),
            schema: output_schema(briefs),
        },
        StructuredOutput::JsonObject => OutputMode::JsonObject,
        StructuredOutput::Text => OutputMode::Text,
    }
}

fn add_usage(a: Option<Usage>, b: Option<Usage>) -> Option<Usage> {
    match (a, b) {
        (None, None) => None,
        (a, b) => {
            let a = a.unwrap_or(Usage {
                input_tokens: 0,
                output_tokens: 0,
            });
            let b = b.unwrap_or(Usage {
                input_tokens: 0,
                output_tokens: 0,
            });
            Some(Usage {
                input_tokens: a.input_tokens.saturating_add(b.input_tokens),
                output_tokens: a.output_tokens.saturating_add(b.output_tokens),
            })
        }
    }
}

/// The plain-English problem a repair asks the model to fix, for the violations that are the
/// model's to fix: text and email rules. Media and option violations are not.
fn problem(brief: &TargetBrief, v: &Violation) -> Option<String> {
    let a = brief.account;
    let number = |key: &str| v.params.get(key).and_then(Value::as_u64);
    Some(match v.code {
        ViolationCode::TextRequired => format!("Target {a}: the text is empty; write it"),
        ViolationCode::TextTooLong => {
            let actual = number("actual").unwrap_or_default();
            let max = number("max").unwrap_or_else(|| u64::from(brief.text_max));
            let counted = if brief.includes_link_and_tags {
                ", counted with the link and the tags that are added after the text"
            } else {
                ""
            };
            format!(
                "Target {a}: text is {actual} {}; the limit is {max}{counted}",
                measure_unit(brief.measure)
            )
        }
        ViolationCode::EmailDocumentRequired => {
            format!("Target {a}: the email is missing; give a subject, a preheader, and paragraphs")
        }
        ViolationCode::EmailSubjectRequired => format!("Target {a}: the email has no subject"),
        ViolationCode::EmailTooLarge => {
            format!("Target {a}: the email is too long; shorten its paragraphs")
        }
        ViolationCode::EmailMergeTagInvalid => {
            format!("Target {a}: remove the {{{{...}}}} placeholders from the email")
        }
        _ => return None,
    })
}

impl DrafterService {
    /// Drafts text for each target, validated against the target's limits, with one repair
    /// when the model's answer is not usable. Nothing is published and no post is created.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] below `edit`; [`DrafterError::Validation`] for a request
    /// over its limits, media that is not ready, or a reply that cannot be answered;
    /// [`DrafterError::NotFound`] for a target, media, or comment outside the scope;
    /// [`DrafterError::ProviderRequired`] when no provider is given and the owner has no
    /// default, or the given one is gone; [`DrafterError::ProviderFailed`] when the provider
    /// fails, or its answer is unusable twice (`invalid_output`); and [`DrafterError::Data`].
    #[allow(clippy::too_many_lines)] // the ten steps read best in order
    pub async fn generate(
        &self,
        scope: &OwnerScope,
        request: GenerateRequest,
    ) -> Result<Generation, DrafterError> {
        // 1. Limits and capability, before anything is read.
        require(scope, Capability::Edit)?;
        let deadline = Instant::now() + self.inner.settings.request_timeout;
        check_request(&request)?;

        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        // 2. The targets, through the scope's account limit.
        let (targets, mut violations) = self
            .load_targets(&mut conn, scope, &request.targets)
            .await?;
        // 3. The media.
        let media = if request.media_ids.is_empty() {
            Vec::new()
        } else {
            MediaRepo::get_many(&mut conn, scope, &request.media_ids)
                .await
                .map_err(data(Capability::Edit))?
        };
        for (i, m) in media.iter().enumerate() {
            if m.status != MediaStatus::Ready {
                violations.push(violation(
                    format!("media_ids[{i}]"),
                    "ai.media_not_ready",
                    "This media is still being processed, or its upload failed. Wait for it, or remove it.",
                ));
            }
        }
        if !violations.is_empty() {
            return Err(DrafterError::Validation(violations));
        }
        // 4. The comment a reply answers.
        let reply = match (request.mode, request.comment_id, targets.first()) {
            (Mode::Reply, Some(id), Some(target)) => {
                Some(Self::load_reply(&mut conn, scope, id, target.account.id).await?)
            }
            _ => None,
        };
        // 5. The provider and model.
        let row = Self::resolve_provider(&mut conn, scope, request.provider_id).await?;
        let loaded = Loaded::new(row)?;
        let model = match request.model.as_deref().map(str::trim) {
            Some(model) => model.to_owned(),
            None => loaded.config.default_model.clone().ok_or_else(|| {
                DrafterError::Validation(vec![violation(
                    "model",
                    "ai.model_required",
                    "Choose a model, or set a default model for this provider.",
                )])
            })?,
        };
        // 6. The briefs.
        let brand = BrandProfileRepo::get(&mut conn, scope)
            .await
            .map_err(data(Capability::Edit))?
            .map(|b| b.fields);
        let owner_prefs = UserPreferencesRepo::get(&mut conn, scope.owner).await?;
        let actor_prefs = if scope.actor == scope.owner {
            owner_prefs.clone()
        } else {
            UserPreferencesRepo::get(&mut conn, scope.actor).await?
        };
        let store_prompt = owner_prefs.is_some_and(|p| p.store_ai_prompts);
        let ui_locale = actor_prefs.map_or_else(|| DEFAULT_LOCALE.to_owned(), |p| p.locale);
        let assets: Vec<MediaAsset> = media.iter().map(|m| m.to_asset(scope.owner)).collect();
        let items: Vec<MediaItem> = request
            .media_ids
            .iter()
            .map(|id| MediaItem {
                asset: *id,
                alt_text: None,
            })
            .collect();
        let reply_to = reply.as_ref().map(|r| r.to.clone());
        let reply_mode = request.mode == Mode::Reply;
        // Outside reply mode a target with no language of its own gets the interface locale;
        // a reply is in the comment's language, and this is only the one used when that is
        // unclear: the style's, else the brand's, else the interface locale.
        let mut fallback_language = ui_locale.clone();
        let mut briefs = Vec::with_capacity(targets.len());
        for target in &targets {
            let style = Self::style(&mut conn, scope, target.account.id).await?;
            let style_language = style.as_ref().and_then(|s| s.language.as_deref());
            let brand_language = brand.as_ref().and_then(|b| b.default_language.as_deref());
            let language = language_for(
                request.language.as_deref(),
                style_language,
                brand_language,
                reply_mode,
                &ui_locale,
            );
            if reply_mode {
                fallback_language =
                    language_for(None, style_language, brand_language, false, &ui_locale)
                        .unwrap_or_else(|| ui_locale.clone());
            }
            briefs.push(Self::brief(
                target,
                &assets,
                &items,
                request.link.as_ref(),
                reply_to.as_ref(),
                style,
                language,
            ));
        }
        drop(conn);

        // 7. What the model is shown of the media, in its own share of the time so the provider
        // calls keep the rest.
        let media_deadline =
            deadline.min(Instant::now() + self.inner.settings.request_timeout / MEDIA_SHARE);
        let context = self
            .media_context(
                &assets,
                loaded.config.takes_images(&model, loaded.preset),
                media_deadline,
            )
            .await;

        // 8. Assemble and call.
        let input = PromptInput {
            mode: request.mode,
            user_prompt: &request.prompt,
            link: request.link.as_ref(),
            brand: brand.as_ref(),
            targets: &briefs,
            reply: reply.as_ref().map(|r| &r.context),
            fallback_language: &fallback_language,
            media_note: context.note.as_deref(),
        };
        let mut parts = vec![Part::Text(user_message(&input))];
        parts.extend(context.images);
        let mut chat = ChatRequest {
            model: model.clone(),
            system: system_prompt(&input),
            messages: vec![ChatMessage {
                role: Role::User,
                parts,
            }],
            // The provider reads the configured temperature itself.
            temperature: None,
            max_tokens: loaded.config.max_tokens(&model),
            output: output_mode(&loaded, &briefs),
            deadline,
        };
        let plan = Plan {
            mode: request.mode,
            targets,
            briefs,
            assets,
            items,
            link: request.link.clone(),
            reply_to,
        };
        let record = Record {
            provider_id: loaded.row.id,
            model: &model,
            mode: request.mode,
            prompt: &request.prompt,
            store_prompt,
        };
        let provider = self.build(&loaded)?;
        // A failure before any answer spent nothing: no row. That includes `invalid_response`
        // (a redirect, an oversized body, a body not in the protocol's shape): nothing usable
        // came back to repair.
        let first = provider
            .chat(&chat)
            .await
            .map_err(DrafterError::ProviderFailed)?;
        let mut usage = first.usage;
        let mut evaluated = self.evaluate(&plan, &first, None);
        let mut notes = context.notes;

        // 9. One repair, appended to the one user message: chat templates that need the roles
        // to alternate refuse two user turns in a row, and the repair quotes the answer itself.
        if !evaluated.problems.is_empty() {
            let previous = first.text.as_str();
            // A newline keeps it apart from the request where a protocol joins text parts.
            let repair = format!("\n{}", repair_message(previous, &evaluated.problems));
            if let Some(message) = chat.messages.first_mut() {
                message.parts.push(Part::Text(repair));
            }
            match provider.chat(&chat).await {
                // The first answer parsed: it stands, with what is still wrong, rather than a
                // failure that would throw away a paid, readable draft.
                Err(_) if evaluated.checked.is_some() => notes.push(message(
                    "ai.repair_failed",
                    "The model could not be asked to fix its draft, so the draft is shown as \
                     it came, with what is still wrong.",
                )),
                Err(e) => {
                    self.record_generation(scope, &record, usage, GenerationOutcome::Failed)
                        .await?;
                    return Err(DrafterError::ProviderFailed(e));
                }
                Ok(second) => {
                    usage = add_usage(usage, second.usage);
                    // A target the repair left out keeps its first variant: the repair was
                    // asked to change nothing else.
                    let again = self.evaluate(&plan, &second, evaluated.parsed.as_ref());
                    // An unparseable repair keeps a first answer that parsed.
                    if again.checked.is_some() || evaluated.checked.is_none() {
                        evaluated = again;
                    }
                }
            }
        }

        // 10. Record and return.
        let Some((draft, violations)) = evaluated.checked else {
            self.record_generation(scope, &record, usage, GenerationOutcome::InvalidOutput)
                .await?;
            return Err(DrafterError::ProviderFailed(LlmError::new(
                FailureReason::InvalidOutput,
            )));
        };
        let id = self
            .record_generation(scope, &record, usage, GenerationOutcome::Ok)
            .await?;
        Ok(Generation {
            id,
            provider_id: loaded.row.id,
            model,
            draft,
            violations,
            notes,
            usage,
        })
    }

    /// Rewrites `text` for each target with fresh wording: what E1's recurrence tick calls
    /// with the post's text and its targets, on the owner's default provider.
    ///
    /// # Errors
    /// As [`DrafterService::generate`]; a `text` over [`PROMPT_MAX`] characters is a
    /// [`DrafterError::Validation`] with `ai.prompt_too_long`.
    pub async fn rewrite(
        &self,
        scope: &OwnerScope,
        text: &str,
        targets: &[AccountId],
    ) -> Result<Generation, DrafterError> {
        self.generate(
            scope,
            GenerateRequest {
                mode: Mode::Rewrite,
                prompt: text.to_owned(),
                targets: targets.to_vec(),
                language: None,
                media_ids: Vec::new(),
                comment_id: None,
                provider_id: None,
                model: None,
                link: None,
            },
        )
        .await
    }

    /// Links a generation to the post saved from it (D3). Returns whether the owner has such a
    /// generation.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] below `edit`, [`DrafterError::NotFound`] when the owner has
    /// no such post, and [`DrafterError::Data`].
    pub async fn link_generation(
        &self,
        scope: &OwnerScope,
        id: AiGenerationId,
        post: PostId,
    ) -> Result<bool, DrafterError> {
        require(scope, Capability::Edit)?;
        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        AiGenerationRepo::link_post(&mut conn, scope, id, post)
            .await
            .map_err(data(Capability::Edit))
    }

    /// The owner's generations, newest first.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] for a delegate, and [`DrafterError::Data`].
    pub async fn generations(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<AiGeneration>, DrafterError> {
        require(scope, Capability::OwnerOnly)?;
        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        AiGenerationRepo::list(&mut conn, scope, pagination)
            .await
            .map_err(data(Capability::OwnerOnly))
    }

    /// Step 2: each target through the scope (outside the limit is `NotFound`), with its
    /// platform's agent; a platform this server does not run is a violation.
    async fn load_targets(
        &self,
        conn: &mut PgConnection,
        scope: &OwnerScope,
        ids: &[AccountId],
    ) -> Result<(Vec<Target>, Vec<Violation>), DrafterError> {
        let mut targets = Vec::with_capacity(ids.len());
        let mut violations = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let account = AccountRepo::get(conn, scope, *id)
                .await
                .map_err(data(Capability::Edit))?;
            match self.inner.registry.get(&account.platform) {
                Some(agent) => targets.push(Target {
                    account,
                    agent: Arc::clone(agent),
                }),
                None => violations.push(violation(
                    format!("targets[{i}]"),
                    "ai.platform_unavailable",
                    "This account's platform is not available on this server.",
                )),
            }
        }
        Ok((targets, violations))
    }

    /// Step 4: the inbox item, which must be on `account` and name the post it is on, and the
    /// text that was published there.
    async fn load_reply(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: InboxItemId,
        account: AccountId,
    ) -> Result<Reply, DrafterError> {
        let item = InboxItemRepo::get(conn, scope, id)
            .await
            .map_err(data(Capability::Edit))?
            .ok_or(DrafterError::NotFound)?;
        let mut violations = Vec::new();
        if item.account_id != account {
            violations.push(violation(
                "targets[0]",
                "ai.reply_account_mismatch",
                "Draft the reply for the account the comment is on.",
            ));
        }
        if item.remote_post_id.is_none() {
            violations.push(violation(
                "comment_id",
                "ai.reply_target_unknown",
                "postit does not know which post this comment is on, so it cannot be answered.",
            ));
        }
        let Some(remote_post_id) = item.remote_post_id.filter(|_| violations.is_empty()) else {
            return Err(DrafterError::Validation(violations));
        };
        let original_text = match item.delivery_id {
            Some(delivery) => Self::published_text(conn, scope, delivery, account).await?,
            None => None,
        };
        Ok(Reply {
            context: ReplyContext {
                original_text,
                author: item.author_name.or(item.author_handle),
                comment: item.text,
            },
            to: ReplyTo {
                remote_post_id,
                remote_comment_id: Some(item.remote_comment_id),
                url: None,
            },
        })
    }

    /// What `delivery` published to `account`: its job's frozen variant, so a later edit of
    /// the post does not change it. `None` when the delivery or the variant is gone.
    async fn published_text(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        delivery: DeliveryId,
        account: AccountId,
    ) -> Result<Option<String>, DrafterError> {
        let delivery = match DeliveryRepo::get(conn, scope, delivery).await {
            Ok(d) => d,
            Err(DataError::NotFound) => return Ok(None),
            Err(e) => return Err(data(Capability::Edit)(e)),
        };
        match JobRepo::snapshot(conn, delivery.job_id).await {
            Ok(snapshot) => Ok(snapshot.variant_for(account).map(|v| v.text.clone())),
            Err(DataError::NotFound) => Ok(None),
            Err(e) => Err(data(Capability::Edit)(e)),
        }
    }

    /// The account's writing style, when it has one.
    async fn style(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        account: AccountId,
    ) -> Result<Option<WritingStyleFields>, DrafterError> {
        match WritingStyleRepo::get(conn, scope, account).await {
            Ok(style) => Ok(Some(style.fields)),
            Err(DataError::NotFound) => Ok(None),
            Err(e) => Err(data(Capability::Edit)(e)),
        }
    }

    /// Step 6: the target's brief, from the format `pick_format` chooses for a probe variant
    /// carrying the request's media, so a photo post is written to the caption limit. When it
    /// refuses (a media problem, which validation reports), the `Text` format or the first.
    fn brief(
        target: &Target,
        assets: &[MediaAsset],
        items: &[MediaItem],
        link: Option<&Url>,
        reply_to: Option<&ReplyTo>,
        style: Option<WritingStyleFields>,
        language: Option<String>,
    ) -> TargetBrief {
        let descriptor = target.agent.descriptor();
        let is_email = target.is_email();
        let probe = Variant {
            target: target.account_ref(),
            kind: if reply_to.is_some() {
                PostKind::Comment
            } else {
                PostKind::Post
            },
            text: String::new(),
            media: items.to_vec(),
            link: link.cloned(),
            tags: Vec::new(),
            disclose_ai_generated: true,
            reply_to: reply_to.cloned(),
            email: is_email.then(|| EmailDocument {
                subject: String::new(),
                preheader: None,
                language: EmailLanguage::EnUs,
                blocks: Vec::new(),
            }),
            options: Map::new(),
        };
        let resolved: Vec<&MediaAsset> = assets.iter().collect();
        // The registry refuses a descriptor with no formats, so one is always found.
        let format = pick_format(descriptor, &probe, &resolved)
            .ok()
            .or_else(|| descriptor.formats.iter().find(|f| f.id == FormatId::Text))
            .or_else(|| descriptor.formats.first());
        let text = format.map_or(
            TextRules {
                measure: TextMeasure::Graphemes,
                max: 0,
                required: false,
                includes_link_and_tags: false,
            },
            |f| f.text,
        );
        TargetBrief {
            account: target.account.id,
            platform_name: descriptor.display_name.clone(),
            is_email,
            text_max: text.max,
            measure: text.measure,
            includes_link_and_tags: text.includes_link_and_tags,
            style,
            language,
        }
    }

    /// Step 7: stills of the first [`IMAGES_MAX`] images and [`FRAMES_PER_VIDEO`] frames of each
    /// video, in the post's order and at most [`PICTURES_MAX`] in all, when the model takes
    /// images; otherwise a note. The pictures are shared out first and every file is then
    /// prepared at once; one that cannot be prepared before `deadline` is left out with a note.
    /// A video this server cannot cut is described by its length alone, with no note.
    async fn media_context(
        &self,
        assets: &[MediaAsset],
        takes_images: bool,
        deadline: Instant,
    ) -> MediaContext {
        let mut context = MediaContext::default();
        if assets.is_empty() {
            return context;
        }
        if !takes_images {
            return ignored_media(assets);
        }
        let (jobs, left_out) = plan_media(assets);
        let frames = &self.inner.frames;
        let ready = futures::future::join_all(jobs.into_iter().map(|job| async move {
            match job {
                Job::Still(asset) => Ready::Still(
                    tokio::time::timeout_at(deadline, frames.still(asset))
                        .await
                        .ok()
                        .and_then(Result::ok),
                ),
                Job::Video(asset, 0) => Ready::Video(asset, 0, None),
                Job::Video(asset, wanted) => Ready::Video(
                    asset,
                    wanted,
                    tokio::time::timeout_at(deadline, frames.frames(asset, wanted))
                        .await
                        .ok()
                        .and_then(Result::ok),
                ),
            }
        }))
        .await;

        let mut lines = Vec::new();
        let (mut stills, mut unreadable) = (0_usize, 0_usize);
        let mut truncated = false;
        for item in ready {
            match item {
                Ready::Still(Some(still)) => {
                    stills += 1;
                    context.images.push(Part::Image { jpeg: still.jpeg });
                }
                Ready::Still(None) => unreadable += 1,
                Ready::Video(asset, wanted, cut) => {
                    if wanted == 0 {
                        truncated = true;
                    } else if cut.is_none() {
                        unreadable += 1;
                    }
                    let (line, shown) = video_shown(asset, wanted, cut);
                    lines.push(line);
                    context.images.extend(shown);
                }
            }
        }
        if stills > 0 {
            lines.insert(
                0,
                format!(
                    "{} from the post {} attached, in the post's order.",
                    plural(stills, "image", "images"),
                    if stills == 1 { "is" } else { "are" }
                ),
            );
        }
        if left_out > 0 {
            lines.push(format!(
                "{} more on the post {} not shown.",
                plural(left_out, "image", "images"),
                if left_out == 1 { "is" } else { "are" }
            ));
            truncated = true;
        }
        if truncated {
            context.notes.push(message(
                "ai.media_truncated",
                "Only part of your media was shown to the model: the first 4 images, and 8 \
                 pictures in all with video frames.",
            ));
        }
        if unreadable > 0 {
            context.notes.push(message(
                "ai.media_unreadable",
                "Some media could not be prepared for the model, so it was not shown to it.",
            ));
        }
        context.note = Some(lines.join("\n"));
        context
    }

    /// Steps 8 and 9 for one answer: parse it, resolve and validate each target's variant, and
    /// list what a repair must fix. A target the answer left out takes its variant from
    /// `earlier`, the answer being repaired, when that has one. An empty answer (empty
    /// `choices`, say), or one with no variant for any target, cannot be used.
    fn evaluate(&self, plan: &Plan, answer: &ChatResponse, earlier: Option<&Parsed>) -> Evaluated {
        let mut problems = Vec::new();
        match answer.finish {
            Finish::Length => problems.push(CUT_OFF.to_owned()),
            Finish::Refused => problems.push(REFUSED.to_owned()),
            Finish::Stop | Finish::Other => {}
        }
        if answer.text.trim().is_empty() {
            problems.push(UNREADABLE.to_owned());
            return Evaluated {
                parsed: None,
                checked: None,
                problems,
            };
        }
        let Ok(mut parsed) = parse(&answer.text, &plan.briefs) else {
            problems.push(NOT_JSON.to_owned());
            return Evaluated {
                parsed: None,
                checked: None,
                problems,
            };
        };
        if let Some(earlier) = earlier {
            parsed.fill_from(earlier);
        }
        if parsed.variants.is_empty() {
            problems.push(NO_TARGET.to_owned());
            return Evaluated {
                parsed: None,
                checked: None,
                problems,
            };
        }
        let mut draft = to_draft(
            &parsed,
            &plan.briefs,
            &plan.items,
            plan.link.clone(),
            plan.reply_to.clone(),
            |account| plan.email_language(account),
        );
        if plan.mode == Mode::Reply {
            // A reply carries no hashtags.
            draft.post.tags.clear();
            for over in draft.overrides.values_mut() {
                over.tags = Some(Vec::new());
            }
        }
        let now = self.inner.clock.now();
        let mut violations = BTreeMap::new();
        for (target, brief) in plan.targets.iter().zip(&plan.briefs) {
            let account = brief.account;
            let missing = parsed.missing.contains(&account);
            let mut found = Vec::new();
            if missing {
                found.push(violation(
                    "text",
                    "ai.target_missing",
                    "The model wrote nothing for this account. Write it yourself, or try again.",
                ));
                problems.push(format!("Target {account}: no variant was returned"));
            }
            match resolve_variant(
                &draft.post,
                &target.account_ref(),
                draft.overrides.get(&account),
            ) {
                Ok(variant) => {
                    let descriptor = target.agent.descriptor();
                    let checked = validate_target(&ValidationInput {
                        descriptor,
                        options_schema: &descriptor.options_schema,
                        variant: &variant,
                        media: &plan.assets,
                        schedule: Schedule::Now,
                        now,
                    });
                    if !missing {
                        problems.extend(checked.iter().filter_map(|v| problem(brief, v)));
                    }
                    found.extend(checked);
                }
                Err(refused) => found.extend(refused),
            }
            if !found.is_empty() {
                violations.insert(account, found);
            }
        }
        Evaluated {
            parsed: Some(parsed),
            checked: Some((draft, violations)),
            problems,
        }
    }

    /// Step 10: the generation row, with the prompt only when the owner keeps prompts and the
    /// keyed hash always.
    async fn record_generation(
        &self,
        scope: &OwnerScope,
        record: &Record<'_>,
        usage: Option<Usage>,
        outcome: GenerationOutcome,
    ) -> Result<AiGenerationId, DrafterError> {
        let id = AiGenerationId::from_uuid(self.inner.ids.generate());
        let tokens = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        AiGenerationRepo::insert(
            &mut conn,
            scope,
            NewAiGeneration {
                id,
                provider_id: Some(record.provider_id),
                model: record.model.to_owned(),
                mode: record.mode.as_str().to_owned(),
                prompt_hash: prompt_hash(
                    self.inner.prompt_key.expose_secret(),
                    scope.owner,
                    record.prompt,
                ),
                prompt: record.store_prompt.then(|| record.prompt.to_owned()),
                input_tokens: usage.map(|u| tokens(u.input_tokens)),
                output_tokens: usage.map(|u| tokens(u.output_tokens)),
                outcome,
            },
        )
        .await
        .map_err(data(Capability::Edit))?;
        Ok(id)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{EmailLanguage, Post, PostOverride, PostOverrides};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn debug_hides_the_users_content() {
        let account = AccountId::from_uuid(Uuid::from_u128(1));
        let request = GenerateRequest {
            mode: Mode::Post,
            prompt: "SECRET-PROMPT".to_owned(),
            targets: vec![account],
            language: Some("SECRET-LANGUAGE".to_owned()),
            media_ids: Vec::new(),
            comment_id: None,
            provider_id: None,
            model: Some("m-1".to_owned()),
            link: Some(Url::parse("https://secret-link.example/").unwrap()),
        };
        let mut overrides = PostOverrides::new();
        overrides.insert(
            account,
            PostOverride {
                text: Some("SECRET-OVERRIDE".to_owned()),
                tags: Some(vec!["SECRETTAG".to_owned()]),
                email: Some(EmailDocument {
                    subject: "SECRET-SUBJECT".to_owned(),
                    preheader: None,
                    language: EmailLanguage::EnUs,
                    blocks: Vec::new(),
                }),
                ..PostOverride::default()
            },
        );
        let generation = Generation {
            id: AiGenerationId::from_uuid(Uuid::from_u128(2)),
            provider_id: AiProviderId::from_uuid(Uuid::from_u128(3)),
            model: "m-1".to_owned(),
            draft: Draft {
                post: Post {
                    kind: PostKind::Post,
                    text: "SECRET-TEXT".to_owned(),
                    media: Vec::new(),
                    link: Some(Url::parse("https://secret-link.example/").unwrap()),
                    tags: vec!["SECRETTAG".to_owned()],
                    disclose_ai_generated: true,
                    reply_to: None,
                },
                overrides,
            },
            violations: BTreeMap::new(),
            notes: vec![message("ai.media_ignored", "x")],
            usage: None,
        };
        let shown = format!("{request:?} {generation:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
        assert!(!shown.contains("secret-link"), "{shown}");
        assert!(shown.contains("prompt_chars: 13"), "{shown}");
        assert!(
            shown.contains("m-1") && shown.contains("ai.media_ignored"),
            "{shown}"
        );
    }

    #[test]
    fn usage_sums_what_is_known() {
        let u = |i, o| Usage {
            input_tokens: i,
            output_tokens: o,
        };
        assert_eq!(add_usage(None, None), None);
        assert_eq!(add_usage(Some(u(1, 2)), None), Some(u(1, 2)));
        assert_eq!(add_usage(Some(u(1, 2)), Some(u(3, 4))), Some(u(4, 6)));
        assert_eq!(
            add_usage(Some(u(u32::MAX, 0)), Some(u(1, 0))),
            Some(u(u32::MAX, 0))
        );
    }
}
