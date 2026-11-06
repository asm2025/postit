//! `DrafterService`: provider management (this module) and generation.
//!
//! Provider configuration is owner-only; listing providers and models needs `edit`. A key or a
//! header value is sealed before it is stored, opened only to hand it to the provider factory
//! (or to keep the half an update does not replace), and never put in a view, an error, a log
//! line, or an audit detail.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use postit_config::AiSettings;
use postit_core::{
    AccountId, AgentRegistry, AiGenerationId, AiProviderId, AuditEventId, Clock, FrameExtractor,
    IdGenerator, InboxItemId, MediaAssetId, Message, Violation,
};
use postit_data::ai_providers::{AiProvider, AiProviderChange, AiProviderRepo, NewAiProvider};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::{Access, Capability, DataError, OwnerScope};
use postit_vault::{Sealed, Vault};
use reqwest::header::{HeaderName, HeaderValue};
use secrecy::{ExposeSecret as _, SecretSlice, SecretString};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use tokio::time::Instant;
use url::Url;

use crate::config::{
    ModelConfig, Protocol, ProviderConfig, ProviderSecret, normalize_base, parse_pasted,
    validate_base_url,
};
use crate::error::{DrafterError, FailureReason};
use crate::factory::ProviderFactory;
use crate::output::Draft;
use crate::presets::{Preset, preset, presets};
use crate::prompt::Mode;
use crate::provider::{ChatMessage, ChatRequest, LlmProvider, OutputMode, Part, Role, Usage};

/// `provider_key` of a provider pasted as an opencode entry.
pub const CUSTOM_PROVIDER_KEY: &str = "custom";

/// The connection test's prompt.
pub const TEST_PROMPT: &str = "Reply with OK";

/// The connection test's output token limit.
pub const TEST_MAX_TOKENS: u32 = 5;

/// Longest provider name, in characters.
const NAME_MAX: usize = 100;

/// Longest model ID, in characters.
const MODEL_MAX: usize = 200;

/// Longest API key or header value, in bytes.
const SECRET_MAX: usize = 8_192;

/// Name of the setting a local preset needs.
const PRIVATE_SETTING: &str = "ai.allow_private_endpoints";

/// What [`DrafterService`] needs.
pub struct DrafterDeps {
    /// The database.
    pub pool: PgPool,
    /// Seals and opens provider secrets, and derives the prompt hash key.
    pub vault: Arc<Vault>,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// ID source.
    pub ids: Arc<dyn IdGenerator>,
    /// The platforms, for each target's formats and limits.
    pub registry: Arc<AgentRegistry>,
    /// Stills and video frames for the model to look at.
    pub frames: Arc<dyn FrameExtractor>,
    /// Builds providers from stored rows.
    pub factory: Arc<dyn ProviderFactory>,
    /// `[ai]` settings.
    pub settings: AiSettings,
}

/// The AI drafter. Cloning is cheap.
#[derive(Clone)]
pub struct DrafterService {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) pool: PgPool,
    pub(crate) vault: Arc<Vault>,
    #[allow(dead_code)] // read by generation
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdGenerator>,
    #[allow(dead_code)] // read by generation
    pub(crate) registry: Arc<AgentRegistry>,
    #[allow(dead_code)] // read by generation
    pub(crate) frames: Arc<dyn FrameExtractor>,
    pub(crate) factory: Arc<dyn ProviderFactory>,
    pub(crate) settings: AiSettings,
    /// The `prompt_hash` key, derived once from the vault.
    #[allow(dead_code)] // read by generation
    pub(crate) prompt_key: SecretSlice<u8>,
}

/// One catalog entry as clients see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PresetView {
    /// The preset key.
    pub key: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Wire protocol.
    pub protocol: Protocol,
    /// Prefilled base URL.
    pub base_url: &'static str,
    /// Where the user gets a key.
    pub guide_url: &'static str,
    /// A server on the user's own machine.
    pub local: bool,
    /// Whether it can be added here: false for local presets while
    /// `ai.allow_private_endpoints` is off.
    pub available: bool,
    /// A consumer subscription from the same company cannot be used; the guide says so.
    pub subscription_note: bool,
}

/// Where a listed model comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    /// The provider's config only.
    Config,
    /// The provider's live list only.
    Remote,
    /// Both.
    Both,
}

/// One model of a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelView {
    /// The model ID.
    pub id: String,
    /// Display name, when known.
    pub name: Option<String>,
    /// Whether the model is sent images.
    pub images: bool,
    /// Where it comes from.
    pub source: ModelSource,
}

/// A provider as its caller may see it: the owner sees the config without secrets, a delegate
/// only names and models.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum ProviderView {
    /// The owner's view.
    Owner {
        /// The provider.
        id: AiProviderId,
        /// The preset key, or `custom`.
        preset: String,
        /// Display name.
        name: String,
        /// Wire protocol.
        protocol: Protocol,
        /// Where requests go.
        base_url: Option<Url>,
        /// Names of the headers whose values are sealed.
        header_names: Vec<String>,
        /// Whether a sealed secret (the key, header values, or both) is stored.
        has_key: bool,
        /// The configured models.
        models: Vec<ModelView>,
        /// The model used when a request names none.
        default_model: Option<String>,
        /// Whether it is the owner's default.
        is_default: bool,
    },
    /// A delegate's view.
    Delegate {
        /// The provider.
        id: AiProviderId,
        /// The preset key, or `custom`.
        preset: String,
        /// Display name.
        name: String,
        /// The configured models.
        models: Vec<ModelView>,
        /// The model used when a request names none.
        default_model: Option<String>,
        /// Whether it is the owner's default.
        is_default: bool,
    },
}

/// A provider to add.
pub enum CreateProvider {
    /// From the catalog.
    Preset {
        /// The preset key.
        key: String,
        /// The API key; required unless the preset is local.
        api_key: Option<SecretString>,
        /// The model used when a request names none.
        default_model: Option<String>,
    },
    /// From a pasted opencode `provider` entry, which may carry the key and header values.
    Pasted {
        /// The entry as pasted.
        entry: Value,
    },
}

impl fmt::Debug for CreateProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preset {
                key,
                api_key,
                default_model,
            } => f
                .debug_struct("Preset")
                .field("key", key)
                .field("has_api_key", &api_key.is_some())
                .field("default_model", default_model)
                .finish(),
            Self::Pasted { .. } => f.debug_struct("Pasted").finish_non_exhaustive(),
        }
    }
}

/// Changes to a provider. `None` keeps a part as it is.
#[derive(Debug, Default)]
pub struct UpdateProvider {
    /// New display name.
    pub name: Option<String>,
    /// New base URL; checked and normalized for the protocol.
    pub base_url: Option<Url>,
    /// New default model; added to the models when missing.
    pub default_model: Option<String>,
    /// Replaces every configured model.
    pub models: Option<BTreeMap<String, ModelConfig>>,
    /// Replaces the API key. There is no way to clear it short of replacing it.
    pub api_key: Option<SecretString>,
    /// Replaces every header value and the header names kept in the config.
    pub headers: Option<BTreeMap<String, SecretString>>,
    /// Makes the provider the owner's default.
    pub make_default: bool,
}

/// A provider's models: the configured ones merged with its live list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelList {
    /// Configured models first, then listed ones the config does not name.
    pub models: Vec<ModelView>,
    /// Why the live list could not be read; the configured models are still returned.
    pub remote_error: Option<FailureReason>,
}

/// What a connection test found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TestResult {
    /// Whether the provider answered.
    pub ok: bool,
    /// Why not.
    pub reason: Option<FailureReason>,
    /// How many models the live list holds, when it was read.
    pub models_found: Option<usize>,
}

/// A generation request.
#[derive(Debug, Clone)]
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

/// A generation's result.
#[derive(Debug, Clone)]
pub struct Generation {
    /// The generation row.
    pub id: AiGenerationId,
    /// The provider used.
    pub provider_id: AiProviderId,
    /// The model used.
    pub model: String,
    /// The draft.
    pub draft: Draft,
    /// What is still wrong, per target.
    pub violations: BTreeMap<AccountId, Vec<Violation>>,
    /// Notes for the user.
    pub notes: Vec<Message>,
    /// Tokens spent, summed over every call.
    pub usage: Option<Usage>,
}

fn violation(path: impl Into<String>, key: &'static str, en: &'static str) -> Violation {
    Violation::agent_rule(Message::new_static(key, en)).at(path)
}

fn require(scope: &OwnerScope, capability: Capability) -> Result<(), DrafterError> {
    scope
        .require(capability)
        .map_err(|_| DrafterError::Forbidden(capability))
}

/// Maps a repository error: a scope refusal is `Forbidden(capability)`.
fn data(capability: Capability) -> impl Fn(DataError) -> DrafterError {
    move |e| match e {
        DataError::Scope(_) => DrafterError::Forbidden(capability),
        DataError::NotFound => DrafterError::NotFound,
        other => DrafterError::Data(other),
    }
}

fn corrupt(what: &str) -> DrafterError {
    DrafterError::Data(DataError::Conflict(format!("ai_provider_{what}")))
}

/// Whether a key or a string anywhere inside `value` holds a NUL, which Postgres refuses in
/// `jsonb` with an error that can quote the value.
fn has_nul(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains('\0'),
        Value::Array(items) => items.iter().any(has_nul),
        Value::Object(map) => map.iter().any(|(k, v)| k.contains('\0') || has_nul(v)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// A display name, trimmed.
fn check_name(name: &str, path: &str, violations: &mut Vec<Violation>) -> String {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX || name.contains('\0') {
        violations.push(violation(
            path,
            "ai.name_invalid",
            "Give the provider a name of 1 to 100 characters.",
        ));
    }
    name.to_owned()
}

/// A model ID, trimmed.
fn check_model(model: &str, path: &str, violations: &mut Vec<Violation>) -> String {
    let model = model.trim();
    if model.is_empty() || model.chars().count() > MODEL_MAX || model.contains('\0') {
        violations.push(violation(
            path,
            "ai.model_invalid",
            "Give a model ID of 1 to 200 characters.",
        ));
    }
    model.to_owned()
}

/// Checks that the key and header values can be sent as HTTP headers.
fn check_secret(
    secret: &ProviderSecret,
    key_path: &str,
    header_path: &str,
    violations: &mut Vec<Violation>,
) {
    if let Some(key) = &secret.api_key {
        let key = key.expose_secret();
        if key.trim().is_empty()
            || key.len() > SECRET_MAX
            || HeaderValue::from_str(&format!("Bearer {key}")).is_err()
        {
            violations.push(violation(
                key_path,
                "ai.api_key_invalid",
                "The key holds characters that cannot be sent. Paste it again.",
            ));
        }
    }
    for (name, value) in &secret.headers {
        let path = format!("{header_path}.{name}");
        if HeaderName::from_bytes(name.as_bytes()).is_err() {
            violations.push(violation(
                path,
                "ai.header_name_invalid",
                "A header name may hold only letters, digits, and dashes.",
            ));
        } else if value.expose_secret().len() > SECRET_MAX
            || HeaderValue::from_str(value.expose_secret()).is_err()
        {
            violations.push(violation(
                path,
                "ai.header_value_invalid",
                "The header value holds characters that cannot be sent.",
            ));
        }
    }
}

fn ensure_default_listed(config: &mut ProviderConfig) {
    if let Some(model) = &config.default_model {
        config.models.entry(model.clone()).or_default();
    }
}

/// A provider row with its config read.
struct Loaded {
    row: AiProvider,
    config: ProviderConfig,
    preset: Option<&'static Preset>,
}

impl Loaded {
    fn new(row: AiProvider) -> Result<Self, DrafterError> {
        let config = serde_json::from_value(row.config.clone()).map_err(|_| corrupt("config"))?;
        let preset = preset(&row.provider_key);
        Ok(Self {
            row,
            config,
            preset,
        })
    }

    fn name(&self) -> String {
        self.config
            .name
            .clone()
            .or_else(|| self.preset.map(|p| p.name.to_owned()))
            .unwrap_or_else(|| self.row.provider_key.clone())
    }

    fn lists_models(&self) -> bool {
        self.preset.is_none_or(|p| p.lists_models)
    }

    fn protocol(&self) -> Protocol {
        self.config
            .protocol()
            .ok()
            .or(self.preset.map(|p| p.protocol))
            .unwrap_or(Protocol::OpenAiCompatible)
    }

    fn model_view(&self, id: &str, name: Option<String>, source: ModelSource) -> ModelView {
        ModelView {
            id: id.to_owned(),
            name,
            images: self.config.takes_images(id, self.preset),
            source,
        }
    }

    fn config_models(&self) -> Vec<ModelView> {
        self.config
            .models
            .iter()
            .map(|(id, m)| self.model_view(id, m.name.clone(), ModelSource::Config))
            .collect()
    }

    fn view(&self, scope: &OwnerScope) -> ProviderView {
        let id = self.row.id;
        let preset = self.row.provider_key.clone();
        let name = self.name();
        let models = self.config_models();
        let default_model = self.config.default_model.clone();
        let is_default = self.row.is_default;
        match scope.access {
            Access::Owner => ProviderView::Owner {
                id,
                preset,
                name,
                protocol: self.protocol(),
                base_url: self.config.options.base_url.clone(),
                header_names: self.config.options.headers.clone(),
                has_key: self.row.secret.is_some(),
                models,
                default_model,
                is_default,
            },
            Access::Delegated { .. } => ProviderView::Delegate {
                id,
                preset,
                name,
                models,
                default_model,
                is_default,
            },
        }
    }

    fn audit_details(&self) -> [(&'static str, Value); 3] {
        [
            (
                "provider_id",
                Value::String(self.row.id.as_uuid().to_string()),
            ),
            ("preset", Value::String(self.row.provider_key.clone())),
            ("name", Value::String(self.name())),
        ]
    }
}

/// What an update changes.
struct Applied {
    /// The new config, when it differs.
    config: Option<Value>,
    /// Whether anything but the header names differs.
    settings_changed: bool,
    /// Whether the update replaces the secret.
    replaces_secret: bool,
    /// The replacing secret; `None` when it ends up empty.
    secret: Option<Sealed<Value>>,
}

/// The config as stored, refusing a NUL anywhere in it.
fn stored_config(config: &ProviderConfig) -> Result<Value, DrafterError> {
    let value = serde_json::to_value(config).map_err(|_| corrupt("config"))?;
    if has_nul(&value) {
        return Err(DrafterError::Validation(vec![violation(
            "",
            "ai.text_invalid",
            "Remove the hidden NUL characters from the settings.",
        )]));
    }
    Ok(value)
}

/// An audit event by `scope`'s actor in its owner's workspace, with `details`.
fn audit_event(
    kind: AuditEventKind,
    scope: &OwnerScope,
    details: impl IntoIterator<Item = (&'static str, Value)>,
) -> Result<AuditEvent, DrafterError> {
    let event = AuditEvent::new(kind).actor(scope.actor).owner(scope.owner);
    details
        .into_iter()
        .try_fold(event, |event, (key, value)| event.detail(key, value))
        .map_err(|_| DrafterError::Data(DataError::Conflict("audit_detail".to_owned())))
}

impl DrafterService {
    /// Builds the service and derives the prompt hash key once.
    #[must_use]
    pub fn new(deps: DrafterDeps) -> Self {
        let prompt_key = SecretSlice::from(deps.vault.derive_key("ai_prompt_hash").to_vec());
        Self {
            inner: Arc::new(Inner {
                pool: deps.pool,
                vault: deps.vault,
                clock: deps.clock,
                ids: deps.ids,
                registry: deps.registry,
                frames: deps.frames,
                factory: deps.factory,
                settings: deps.settings,
                prompt_key,
            }),
        }
    }

    /// The preset catalog. Local presets are not `available` while
    /// `ai.allow_private_endpoints` is off.
    #[must_use]
    pub fn presets(&self) -> Vec<PresetView> {
        let allow_private = self.inner.settings.allow_private_endpoints;
        presets()
            .iter()
            .map(|p| PresetView {
                key: p.key,
                name: p.name,
                protocol: p.protocol,
                base_url: p.base_url,
                guide_url: p.guide_url,
                local: p.local,
                available: allow_private || !p.local,
                subscription_note: p.subscription_note,
            })
            .collect()
    }

    /// Adds a provider for the owner. The owner's first provider becomes the default.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] for a delegate, [`DrafterError::Validation`] for an unknown
    /// preset, a missing key, a local preset while private endpoints are off, or a pasted entry
    /// that breaks the rules (nothing is stored), and [`DrafterError::Data`].
    pub async fn create_provider(
        &self,
        scope: &OwnerScope,
        input: CreateProvider,
    ) -> Result<ProviderView, DrafterError> {
        require(scope, Capability::OwnerOnly)?;
        let (provider_key, config, secret) = match input {
            CreateProvider::Preset {
                key,
                api_key,
                default_model,
            } => self.preset_input(&key, api_key, default_model)?,
            CreateProvider::Pasted { entry } => self.paste_input(&entry)?,
        };
        let config_value = stored_config(&config)?;
        let id = AiProviderId::from_uuid(self.inner.ids.generate());
        let secret = self.seal(id, &secret)?;

        let mut tx = self.inner.pool.begin().await.map_err(DataError::from)?;
        let row = AiProviderRepo::create(
            &mut tx,
            scope,
            NewAiProvider {
                id,
                provider_key,
                config: config_value,
                secret,
            },
        )
        .await
        .map_err(data(Capability::OwnerOnly))?;
        let loaded = Loaded::new(row)?;
        let event = audit_event(
            AuditEventKind::AiProviderCreated,
            scope,
            loaded.audit_details(),
        )?;
        self.record(&mut tx, event).await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(loaded.view(scope))
    }

    fn preset_input(
        &self,
        key: &str,
        api_key: Option<SecretString>,
        default_model: Option<String>,
    ) -> Result<(String, ProviderConfig, ProviderSecret), DrafterError> {
        let allow_private = self.inner.settings.allow_private_endpoints;
        let Some(preset) = preset(key) else {
            return Err(DrafterError::Validation(vec![violation(
                "key",
                "ai.preset_unknown",
                "Choose a provider from the list.",
            )]));
        };
        let mut violations = Vec::new();
        if preset.local && !allow_private {
            violations.push(
                violation(
                    "key",
                    "ai.private_endpoints_off",
                    "This provider runs on your own machine, which needs ai.allow_private_endpoints turned on by the administrator.",
                )
                .param("setting", PRIVATE_SETTING),
            );
        }
        if !preset.local && api_key.is_none() {
            violations.push(violation(
                "api_key",
                "ai.api_key_required",
                "Paste the key you got from the provider.",
            ));
        }
        let secret = ProviderSecret {
            api_key,
            headers: BTreeMap::new(),
        };
        check_secret(&secret, "api_key", "headers", &mut violations);
        let default_model =
            default_model.map(|m| check_model(&m, "default_model", &mut violations));
        let base = Url::parse(preset.base_url).map_err(|_| corrupt("preset"))?;
        if violations.is_empty()
            && let Err(v) = validate_base_url(&base, allow_private)
        {
            violations.push(v);
        }
        if !violations.is_empty() {
            return Err(DrafterError::Validation(violations));
        }
        let mut config = ProviderConfig {
            name: Some(preset.name.to_owned()),
            protocol: Some(preset.protocol),
            default_model,
            ..ProviderConfig::default()
        };
        config.options.base_url = Some(normalize_base(preset.protocol, base));
        ensure_default_listed(&mut config);
        Ok((preset.key.to_owned(), config, secret))
    }

    fn paste_input(
        &self,
        entry: &Value,
    ) -> Result<(String, ProviderConfig, ProviderSecret), DrafterError> {
        let (mut config, secret) = parse_pasted(entry, self.inner.settings.allow_private_endpoints)
            .map_err(DrafterError::Validation)?;
        let mut violations = Vec::new();
        let name = config.name.as_deref().map_or_else(
            || "Custom provider".to_owned(),
            |n| check_name(n, "name", &mut violations),
        );
        config.name = Some(name);
        if let Some(model) = config.default_model.take() {
            config.default_model = Some(check_model(&model, "default_model", &mut violations));
        }
        check_secret(
            &secret,
            "options.apiKey",
            "options.headers",
            &mut violations,
        );
        if !violations.is_empty() {
            return Err(DrafterError::Validation(violations));
        }
        ensure_default_listed(&mut config);
        Ok((CUSTOM_PROVIDER_KEY.to_owned(), config, secret))
    }

    /// Changes a provider. A new key or new headers write `ai_provider_key_changed`; any other
    /// change, the default included, writes `ai_provider_updated`. A change that changes nothing
    /// writes nothing.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] for a delegate, [`DrafterError::NotFound`],
    /// [`DrafterError::Validation`], and [`DrafterError::Data`].
    pub async fn update_provider(
        &self,
        scope: &OwnerScope,
        id: AiProviderId,
        change: UpdateProvider,
    ) -> Result<ProviderView, DrafterError> {
        require(scope, Capability::OwnerOnly)?;
        let mut tx = self.inner.pool.begin().await.map_err(DataError::from)?;
        let loaded = self.load(&mut tx, scope, id, Capability::OwnerOnly).await?;
        let default_changed = change.make_default && !loaded.row.is_default;
        let applied = self.apply(&loaded, change)?;
        let replaces_secret = applied.replaces_secret;
        if applied.config.is_some() || replaces_secret {
            AiProviderRepo::update(
                &mut tx,
                scope,
                id,
                AiProviderChange {
                    config: applied.config,
                    secret: replaces_secret.then_some(applied.secret),
                },
            )
            .await
            .map_err(data(Capability::OwnerOnly))?;
        }
        if default_changed {
            AiProviderRepo::set_default(&mut tx, scope, id)
                .await
                .map_err(data(Capability::OwnerOnly))?;
        }
        let after = self.load(&mut tx, scope, id, Capability::OwnerOnly).await?;
        if replaces_secret {
            let event = audit_event(
                AuditEventKind::AiProviderKeyChanged,
                scope,
                after.audit_details(),
            )?;
            self.record(&mut tx, event).await?;
        }
        if applied.settings_changed || default_changed {
            let event = audit_event(
                AuditEventKind::AiProviderUpdated,
                scope,
                after.audit_details(),
            )?;
            self.record(&mut tx, event).await?;
        }
        tx.commit().await.map_err(DataError::from)?;
        Ok(after.view(scope))
    }

    /// The checked config and sealed secret an update leaves.
    fn apply(&self, loaded: &Loaded, change: UpdateProvider) -> Result<Applied, DrafterError> {
        let mut config = loaded.config.clone();
        let mut violations = Vec::new();
        if let Some(name) = &change.name {
            config.name = Some(check_name(name, "name", &mut violations));
        }
        if let Some(url) = change.base_url {
            match validate_base_url(&url, self.inner.settings.allow_private_endpoints) {
                Ok(()) => {
                    config.options.base_url = Some(normalize_base(loaded.protocol(), url));
                }
                Err(mut v) => {
                    v.path = Some("base_url".to_owned());
                    violations.push(v);
                }
            }
        }
        if let Some(models) = change.models {
            config.models = models
                .into_iter()
                .map(|(model, entry)| (check_model(&model, "models", &mut violations), entry))
                .collect();
        }
        if let Some(model) = &change.default_model {
            config.default_model = Some(check_model(model, "default_model", &mut violations));
        }
        ensure_default_listed(&mut config);
        // The header names follow the header values, which are the key change's to report.
        let settings = stored_config(&config)?;
        let mut secret = None;
        if change.api_key.is_some() || change.headers.is_some() {
            let mut opened = self.open(&loaded.row)?;
            if let Some(key) = change.api_key {
                opened.api_key = Some(key);
            }
            if let Some(headers) = change.headers {
                config.options.headers = headers.keys().cloned().collect();
                opened.headers = headers;
            }
            check_secret(&opened, "api_key", "headers", &mut violations);
            secret = Some(opened);
        }
        if !violations.is_empty() {
            return Err(DrafterError::Validation(violations));
        }
        let value = stored_config(&config)?;
        Ok(Applied {
            config: (value != loaded.row.config).then_some(value),
            settings_changed: settings != loaded.row.config,
            replaces_secret: secret.is_some(),
            secret: match secret {
                Some(s) => self.seal(loaded.row.id, &s)?,
                None => None,
            },
        })
    }

    /// Deletes a provider. The owner's generations stay, with no provider; deleting the default
    /// leaves the owner with none.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] for a delegate, [`DrafterError::NotFound`], and
    /// [`DrafterError::Data`].
    pub async fn delete_provider(
        &self,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<(), DrafterError> {
        require(scope, Capability::OwnerOnly)?;
        let mut tx = self.inner.pool.begin().await.map_err(DataError::from)?;
        let loaded = self.load(&mut tx, scope, id, Capability::OwnerOnly).await?;
        if !AiProviderRepo::delete(&mut tx, scope, id)
            .await
            .map_err(data(Capability::OwnerOnly))?
        {
            return Err(DrafterError::NotFound);
        }
        let event = audit_event(
            AuditEventKind::AiProviderDeleted,
            scope,
            loaded.audit_details(),
        )?;
        self.record(&mut tx, event).await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(())
    }

    /// The owner's providers, oldest first, in the view the scope may see.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] below `edit`, and [`DrafterError::Data`].
    pub async fn list_providers(
        &self,
        scope: &OwnerScope,
    ) -> Result<Vec<ProviderView>, DrafterError> {
        require(scope, Capability::Edit)?;
        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        let rows = AiProviderRepo::list(&mut conn, scope)
            .await
            .map_err(data(Capability::Edit))?;
        rows.into_iter()
            .map(|row| Loaded::new(row).map(|l| l.view(scope)))
            .collect()
    }

    /// The provider's configured models merged with its live list, read under one deadline of
    /// `ai.request_timeout`. A preset with no listing is not asked; a failed listing still
    /// returns the configured models, with `remote_error`.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] below `edit`, [`DrafterError::NotFound`], and
    /// [`DrafterError::Data`].
    pub async fn models(
        &self,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<ModelList, DrafterError> {
        require(scope, Capability::Edit)?;
        let loaded = {
            let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
            self.load(&mut conn, scope, id, Capability::Edit).await?
        };
        let mut models = loaded.config_models();
        if !loaded.lists_models() {
            return Ok(ModelList {
                models,
                remote_error: None,
            });
        }
        let provider = self.build(&loaded)?;
        let deadline = Instant::now() + self.inner.settings.request_timeout;
        match provider.models(deadline).await {
            Ok(remote) => {
                for model in remote {
                    match models.iter_mut().find(|m| m.id == model.id) {
                        Some(known) => {
                            known.source = ModelSource::Both;
                            if known.name.is_none() {
                                known.name = model.name;
                            }
                        }
                        None => {
                            models.push(loaded.model_view(
                                &model.id,
                                model.name,
                                ModelSource::Remote,
                            ));
                        }
                    }
                }
                Ok(ModelList {
                    models,
                    remote_error: None,
                })
            }
            Err(e) => Ok(ModelList {
                models,
                remote_error: Some(e.reason),
            }),
        }
    }

    /// Tests the connection under one deadline: a tiny chat (`Reply with OK`, at most 5 output
    /// tokens) to the default model (else the first configured one), then the live model list
    /// when the provider has one. Any successful chat answer counts, whatever its text or
    /// finish, since a thinking model may spend every token thinking. When a chat was sent, it
    /// decides `ok` and a failed listing only leaves `models_found` empty; with no model to
    /// chat with, the listing decides. Writes no generation row.
    ///
    /// # Errors
    /// [`DrafterError::Forbidden`] for a delegate, [`DrafterError::NotFound`], and
    /// [`DrafterError::Data`]. A provider failure is a result with `ok: false`, not an error.
    pub async fn test_provider(
        &self,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<TestResult, DrafterError> {
        require(scope, Capability::OwnerOnly)?;
        let loaded = {
            let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
            self.load(&mut conn, scope, id, Capability::OwnerOnly)
                .await?
        };
        let provider = self.build(&loaded)?;
        let deadline = Instant::now() + self.inner.settings.request_timeout;
        let model = loaded
            .config
            .default_model
            .clone()
            .or_else(|| loaded.config.models.keys().next().cloned());
        let chatted = model.is_some();
        if let Some(model) = model {
            let request = ChatRequest {
                model,
                system: String::new(),
                messages: vec![ChatMessage {
                    role: Role::User,
                    parts: vec![Part::Text(TEST_PROMPT.to_owned())],
                }],
                temperature: None,
                max_tokens: TEST_MAX_TOKENS,
                output: OutputMode::Text,
                deadline,
            };
            if let Err(e) = provider.chat(&request).await {
                return Ok(TestResult {
                    ok: false,
                    reason: Some(e.reason),
                    models_found: None,
                });
            }
        }
        if !loaded.lists_models() {
            return Ok(TestResult {
                ok: chatted,
                reason: (!chatted).then_some(FailureReason::ModelNotFound),
                models_found: None,
            });
        }
        Ok(match provider.models(deadline).await {
            Ok(list) => TestResult {
                ok: true,
                reason: None,
                models_found: Some(list.len()),
            },
            Err(e) => TestResult {
                ok: chatted,
                reason: (!chatted).then_some(e.reason),
                models_found: None,
            },
        })
    }

    /// Drafts text for each target.
    ///
    /// # Errors
    /// [`DrafterError::ProviderRequired`] when no provider is given and the owner has no
    /// default, or the given one is gone.
    pub async fn generate(
        &self,
        scope: &OwnerScope,
        request: GenerateRequest,
    ) -> Result<Generation, DrafterError> {
        require(scope, Capability::Edit)?;
        let _provider = self.resolve_provider(scope, request.provider_id).await?;
        todo!("generation is not built yet")
    }

    /// The provider a generation uses: the given one, else the owner's default. A given ID
    /// that is gone (or another owner's) and a missing default are both `ProviderRequired`.
    pub(crate) async fn resolve_provider(
        &self,
        scope: &OwnerScope,
        id: Option<AiProviderId>,
    ) -> Result<AiProvider, DrafterError> {
        let mut conn = self.inner.pool.acquire().await.map_err(DataError::from)?;
        let row = match id {
            Some(id) => AiProviderRepo::get(&mut conn, scope, id).await,
            None => AiProviderRepo::default(&mut conn, scope).await,
        }
        .map_err(data(Capability::Edit))?;
        row.ok_or(DrafterError::ProviderRequired)
    }

    /// Loads a provider row; a missing one is `NotFound`.
    async fn load(
        &self,
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiProviderId,
        capability: Capability,
    ) -> Result<Loaded, DrafterError> {
        let row = AiProviderRepo::get(conn, scope, id)
            .await
            .map_err(data(capability))?
            .ok_or(DrafterError::NotFound)?;
        Loaded::new(row)
    }

    /// Opens the row's secret straight into the factory.
    fn build(&self, loaded: &Loaded) -> Result<Arc<dyn LlmProvider>, DrafterError> {
        let secret = self.open(&loaded.row)?;
        self.inner
            .factory
            .build(&loaded.config, secret, loaded.preset)
    }

    /// The row's opened secret; an empty one when none is stored.
    fn open(&self, row: &AiProvider) -> Result<ProviderSecret, DrafterError> {
        let Some(sealed) = &row.secret else {
            return Ok(ProviderSecret::default());
        };
        let value: Value = self
            .inner
            .vault
            .open(sealed, &AiProviderRepo::seal_context(row.id))
            .map_err(DataError::from)?;
        ProviderSecret::from_value(&value).ok_or_else(|| corrupt("secret"))
    }

    /// Seals the secret for row `id`; `None` when it holds nothing.
    fn seal(
        &self,
        id: AiProviderId,
        secret: &ProviderSecret,
    ) -> Result<Option<Sealed<Value>>, DrafterError> {
        if secret.api_key.is_none() && secret.headers.is_empty() {
            return Ok(None);
        }
        self.inner
            .vault
            .seal(&secret.to_value(), &AiProviderRepo::seal_context(id))
            .map(Some)
            .map_err(|e| DrafterError::Data(DataError::from(e)))
    }

    async fn record(&self, conn: &mut PgConnection, event: AuditEvent) -> Result<(), DrafterError> {
        let id = AuditEventId::from_uuid(self.inner.ids.generate());
        AuditLog::record(conn, id, event).await?;
        Ok(())
    }
}
