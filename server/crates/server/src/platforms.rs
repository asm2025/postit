//! Platforms at startup (plan 03 A2): the deployment's `[platforms.<id>]` tables resolved
//! against the compiled-in agents, each platform's availability, and the vault and `platforms`
//! startup step.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::Context;
use postit_config::{PlatformTable, Settings};
use postit_core::{AgentRegistry, ConnectMethod, FieldKind, OptionsSchema, PlatformId};
use postit_data::platforms::{PlatformRepo, PlatformRow};
use postit_vault::Vault;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::PgPool;
use url::Url;

use crate::Role;

/// Whether accounts can be connected to a platform in this deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Compiled in, switched on, and configured.
    Available,
    /// A `platforms` row exists but this build has no agent for it.
    NotCompiledIn,
    /// An admin switched the platform off.
    Disabled,
    /// The agent needs a `[platforms.<id>]` table (OAuth, or a required config field) and the
    /// deployment has none.
    NotConfigured,
}

/// The validated `[platforms.<id>]` tables of compiled-in agents, with values coerced to the
/// types their agent's config schema declares. Holds secrets: never log or return it.
#[derive(Default)]
pub struct PlatformConfigs(BTreeMap<PlatformId, Value>);

impl PlatformConfigs {
    /// The resolved table of `id`, an object, when the deployment configures that platform.
    #[must_use]
    pub fn get(&self, id: &PlatformId) -> Option<&Value> {
        self.0.get(id)
    }
}

/// Lists the configured platforms only; values may be secrets.
impl std::fmt::Debug for PlatformConfigs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

/// Resolves `settings.platforms` against `registry`.
///
/// A table for a platform this build does not include is dropped with a warning. Values arrive
/// as strings from TOML or the environment, or already typed by the environment provider; each
/// is turned into a string and coerced to the type of its config field (integers parsed,
/// booleans `true`/`false` in any case, everything else kept as a string), then validated.
///
/// # Errors
///
/// A table key that is not a valid platform ID, or values the agent's config schema rejects.
/// The error names `platforms.<id>.<field>` and the violation code, never a value.
pub fn resolve_platform_configs(
    settings: &Settings,
    registry: &AgentRegistry,
) -> anyhow::Result<PlatformConfigs> {
    resolve_tables(&settings.platforms, registry)
}

fn resolve_tables(
    tables: &BTreeMap<String, PlatformTable>,
    registry: &AgentRegistry,
) -> anyhow::Result<PlatformConfigs> {
    let empty = OptionsSchema::empty();
    let mut out = BTreeMap::new();
    let mut problems = String::new();
    for (key, table) in tables {
        let id = PlatformId::new(key).with_context(|| format!("platforms.{key}"))?;
        let Some(agent) = registry.get(&id) else {
            tracing::warn!(
                platform = %id,
                "a [platforms] table names an agent this build does not include; ignoring it"
            );
            continue;
        };
        let schema = agent.config_schema().unwrap_or(&empty);
        let values = coerce(schema, table.values());
        for violation in schema.validate(&values) {
            let field = violation
                .path
                .as_deref()
                .and_then(|p| p.strip_prefix("options."))
                .unwrap_or("?");
            let _ = write!(
                problems,
                "; platforms.{id}.{field}: {}",
                violation.code.as_str()
            );
        }
        out.insert(id, Value::Object(values));
    }
    if let Some(list) = problems.strip_prefix("; ") {
        anyhow::bail!("invalid platform configuration: {list}");
    }
    Ok(PlatformConfigs(out))
}

/// Converts each value to its field's type; unknown keys and uncoercible values pass through
/// unchanged for validation to report.
fn coerce(schema: &OptionsSchema, values: &Map<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .map(|(name, value)| {
            let kind = schema
                .fields()
                .iter()
                .find(|f| &f.name == name)
                .map(|f| &f.kind);
            let coerced = match (kind, as_text(value)) {
                (Some(kind), Some(text)) => coerce_text(kind, text),
                _ => value.clone(),
            };
            (name.clone(), coerced)
        })
        .collect()
}

/// A scalar as the string it was written as: environment values may arrive typed.
fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn coerce_text(kind: &FieldKind, text: String) -> Value {
    match kind {
        FieldKind::Integer { .. } => text
            .trim()
            .parse::<i64>()
            .map_or(Value::String(text), Value::from),
        FieldKind::Boolean { .. } => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("true") {
                Value::Bool(true)
            } else if trimmed.eq_ignore_ascii_case("false") {
                Value::Bool(false)
            } else {
                Value::String(text)
            }
        }
        FieldKind::String { .. }
        | FieldKind::MultilineText { .. }
        | FieldKind::Enum { .. }
        | FieldKind::DateTime => Value::String(text),
    }
}

/// Whether accounts can be connected to the platform of `row`.
#[must_use]
pub fn availability(
    row: &PlatformRow,
    registry: &AgentRegistry,
    configs: &PlatformConfigs,
) -> Availability {
    let Some(agent) = registry.get(&row.id) else {
        return Availability::NotCompiledIn;
    };
    if !row.enabled {
        return Availability::Disabled;
    }
    let needs_table = matches!(agent.descriptor().connect, ConnectMethod::OAuth(_))
        || agent
            .config_schema()
            .is_some_and(|s| s.fields().iter().any(|f| f.required || f.no_default));
    if needs_table && configs.get(&row.id).is_none() {
        return Availability::NotConfigured;
    }
    Availability::Available
}

/// The OAuth callback a platform's developer app must register:
/// `{public_url}/api/v1/platforms/{id}/oauth/callback`, keeping any base path.
#[must_use]
pub fn oauth_redirect_uri(public_url: &Url, id: &PlatformId) -> Url {
    let mut url = public_url.clone();
    let base = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base}/api/v1/platforms/{id}/oauth/callback"));
    url.set_query(None);
    url.set_fragment(None);
    url
}

/// What the vault and platform startup step needs.
pub struct PlatformStartup<'a> {
    /// The configured vault.
    pub vault: &'a Vault,
    /// Which parts of the process run.
    pub role: Role,
    /// The compiled-in agents.
    pub registry: &'a AgentRegistry,
    /// The resolved platform tables.
    pub configs: &'a PlatformConfigs,
    /// `server.public_url`, the base of OAuth redirect URIs.
    pub public_url: &'a Url,
}

/// Checks the vault against the database in every role. In API roles it then records the
/// compiled-in platforms and logs each unavailable platform and each OAuth redirect URI.
///
/// # Errors
///
/// A vault behind the database's newest key version, a sealed value under a key the vault
/// lacks, or a database failure.
pub async fn check_vault_and_platforms(
    pool: &PgPool,
    startup: &PlatformStartup<'_>,
) -> anyhow::Result<()> {
    postit_data::vault::vault_check_startup(
        pool,
        startup.vault,
        postit_data::vault::SEALED_COLUMNS,
    )
    .await
    .context("checking the vault keys against the database")?;
    if !startup.role.runs_api() {
        return Ok(());
    }
    let registered: Vec<(PlatformId, String)> = startup
        .registry
        .iter()
        .map(|agent| {
            let d = agent.descriptor();
            (d.platform.clone(), d.display_name.clone())
        })
        .collect();
    PlatformRepo::upsert_registered(pool, &registered)
        .await
        .context("recording the registered platforms")?;
    let rows = PlatformRepo::list(pool)
        .await
        .context("listing platforms")?;
    for row in &rows {
        let state = availability(row, startup.registry, startup.configs);
        if state != Availability::Available {
            tracing::warn!(platform = %row.id, availability = ?state, "platform is not available");
        }
    }
    for agent in startup.registry.iter() {
        let d = agent.descriptor();
        if matches!(d.connect, ConnectMethod::OAuth(_)) {
            tracing::info!(
                platform = %d.platform,
                redirect_uri = %oauth_redirect_uri(startup.public_url, &d.platform),
                "register this OAuth redirect URI with the platform's developer app"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::Utc;
    use postit_config::PlatformTable;
    use postit_core::{
        Agent, AgentClient, AgentContext, AgentDescriptor, AgentError, AgentKind, AgentRegistry,
        Capability, ConnectMethod, FormatId, FormatSpec, MediaRules, OAuthProviderSpec,
        OptionsSchema, PlatformId, TextMeasure, TextRules,
    };
    use postit_data::platforms::PlatformRow;
    use serde_json::{Value, json};
    use url::Url;

    use super::*;

    const LBL: &str = r#"{"key":"k","en":"E"}"#;

    fn id(s: &str) -> PlatformId {
        PlatformId::new(s).unwrap()
    }

    fn schema(src: &str) -> OptionsSchema {
        OptionsSchema::from_json_str(&src.replace("LBL", LBL)).unwrap()
    }

    /// Optional `api_version` (integer), `sandbox` (boolean), and `client_id` (string).
    fn optional_schema() -> OptionsSchema {
        schema(
            r#"{"type":"object","properties":{
                "api_version":{"type":"integer","minimum":1,"x-postit-label":LBL},
                "sandbox":{"type":"boolean","x-postit-label":LBL},
                "client_id":{"type":"string","x-postit-label":LBL}
            },"additionalProperties":false}"#,
        )
    }

    fn descriptor(platform: &str, connect: ConnectMethod) -> AgentDescriptor {
        AgentDescriptor {
            platform: id(platform),
            display_name: platform.to_owned(),
            icon_png: &[0x89, b'P', b'N', b'G'],
            brand_color: "#112233".to_owned(),
            kind: AgentKind::Broadcast,
            connect,
            formats: vec![FormatSpec {
                id: FormatId::Text,
                text: TextRules {
                    measure: TextMeasure::Graphemes,
                    max: 500,
                    required: true,
                },
                media: MediaRules::none(),
                selected_by: None,
            }],
            capabilities: std::collections::BTreeSet::new(),
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: OptionsSchema::empty(),
        }
    }

    struct FakeAgent {
        descriptor: AgentDescriptor,
        config: Option<OptionsSchema>,
    }

    #[async_trait]
    impl Agent for FakeAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }

        fn config_schema(&self) -> Option<&OptionsSchema> {
            self.config.as_ref()
        }

        fn create_client(&self, _ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
            Err(AgentError::Unsupported {
                capability: Capability::Commenter,
            })
        }
    }

    fn credential_agent(platform: &str) -> Arc<dyn Agent> {
        Arc::new(FakeAgent {
            descriptor: descriptor(platform, ConnectMethod::Credentials(OptionsSchema::empty())),
            config: Some(optional_schema()),
        })
    }

    fn oauth_agent(platform: &str) -> Arc<dyn Agent> {
        let spec = OAuthProviderSpec::new(
            Url::parse("https://example.test/authorize").unwrap(),
            Url::parse("https://example.test/token").unwrap(),
        );
        Arc::new(FakeAgent {
            descriptor: descriptor(platform, ConnectMethod::OAuth(spec)),
            config: None,
        })
    }

    fn registry(agents: impl IntoIterator<Item = Arc<dyn Agent>>) -> AgentRegistry {
        let mut registry = AgentRegistry::new();
        for agent in agents {
            registry.register(agent).unwrap();
        }
        registry
    }

    fn tables(value: Value) -> BTreeMap<String, PlatformTable> {
        serde_json::from_value(value).unwrap()
    }

    fn row(platform: &str, enabled: bool) -> PlatformRow {
        PlatformRow {
            id: id(platform),
            display_name: platform.to_owned(),
            enabled,
            first_seen_at: Utc::now(),
        }
    }

    #[test]
    fn invalid_platform_table_id_is_an_error() {
        let registry = registry([credential_agent("fake")]);
        let err = resolve_tables(&tables(json!({"Tik-Tok": {}})), &registry).unwrap_err();
        assert!(format!("{err:#}").contains("platforms.Tik-Tok"), "{err:#}");
    }

    #[test]
    fn unknown_platform_table_warns_and_is_dropped() {
        let registry = registry([credential_agent("fake")]);
        let configs = resolve_tables(
            &tables(json!({"tiktok": {"client_id": "abc"}, "fake": {}})),
            &registry,
        )
        .unwrap();
        assert!(configs.get(&id("tiktok")).is_none());
        assert_eq!(configs.get(&id("fake")), Some(&json!({})));
    }

    #[test]
    fn env_strings_coerced_by_schema() {
        let registry = registry([credential_agent("fake")]);
        let configs = resolve_tables(
            &tables(json!({"fake": {"api_version": "3", "sandbox": " TRUE ", "client_id": "c1"}})),
            &registry,
        )
        .unwrap();
        assert_eq!(
            configs.get(&id("fake")),
            Some(&json!({"api_version": 3, "sandbox": true, "client_id": "c1"}))
        );
    }

    /// Values that are typed rather than strings, such as a TOML `api_version = 3` or
    /// `sandbox = false`, go through the same string coercion as string values.
    #[test]
    fn env_values_typed_by_figment_are_coerced_too() {
        let registry = registry([credential_agent("fake")]);
        let configs = resolve_tables(
            &tables(json!({"fake": {"api_version": 3, "sandbox": false, "client_id": 12345}})),
            &registry,
        )
        .unwrap();
        assert_eq!(
            configs.get(&id("fake")),
            Some(&json!({"api_version": 3, "sandbox": false, "client_id": "12345"}))
        );
        let configs =
            resolve_tables(&tables(json!({"fake": {"client_id": true}})), &registry).unwrap();
        assert_eq!(
            configs.get(&id("fake")),
            Some(&json!({"client_id": "true"}))
        );
    }

    #[test]
    fn uncoercible_value_errors_without_echo() {
        let registry = registry([credential_agent("fake")]);
        let err =
            resolve_tables(&tables(json!({"fake": {"sandbox": "1"}})), &registry).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("platforms.fake.sandbox"), "{text}");
        assert!(!text.contains('1'), "{text}");

        let err =
            resolve_tables(&tables(json!({"fake": {"api_version": "x9"}})), &registry).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("platforms.fake.api_version"), "{text}");
        assert!(!text.contains("x9"), "{text}");

        let err =
            resolve_tables(&tables(json!({"fake": {"region": "s3cret"}})), &registry).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("platforms.fake.region"), "{text}");
        assert!(!text.contains("s3cret"), "{text}");
    }

    #[test]
    fn availability_states() {
        let registry = registry([
            credential_agent("fake"),
            oauth_agent("tiktok"),
            credential_agent("plain"),
        ]);
        let configs = resolve_tables(&tables(json!({"fake": {}})), &registry).unwrap();
        assert_eq!(
            availability(&row("fake", true), &registry, &configs),
            Availability::Available
        );
        assert_eq!(
            availability(&row("fake", false), &registry, &configs),
            Availability::Disabled
        );
        assert_eq!(
            availability(&row("gone", true), &registry, &configs),
            Availability::NotCompiledIn
        );
        assert_eq!(
            availability(&row("tiktok", true), &registry, &configs),
            Availability::NotConfigured
        );
        assert_eq!(
            availability(&row("plain", true), &registry, &configs),
            Availability::Available
        );
    }

    #[test]
    fn required_config_field_without_table_is_not_configured() {
        let agent: Arc<dyn Agent> = Arc::new(FakeAgent {
            descriptor: descriptor("needy", ConnectMethod::Credentials(OptionsSchema::empty())),
            config: Some(schema(
                r#"{"type":"object","properties":{
                    "token":{"type":"string","x-postit-label":LBL}
                },"required":["token"],"additionalProperties":false}"#,
            )),
        });
        let registry = registry([agent]);
        let configs = resolve_tables(&BTreeMap::new(), &registry).unwrap();
        assert_eq!(
            availability(&row("needy", true), &registry, &configs),
            Availability::NotConfigured
        );
    }

    #[test]
    fn availability_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(Availability::NotCompiledIn).unwrap(),
            json!("not_compiled_in")
        );
    }

    #[test]
    fn redirect_uri_keeps_base_path() {
        let base = Url::parse("https://h/x/").unwrap();
        assert_eq!(
            oauth_redirect_uri(&base, &id("tiktok")).as_str(),
            "https://h/x/api/v1/platforms/tiktok/oauth/callback"
        );
        let root = Url::parse("https://h").unwrap();
        assert_eq!(
            oauth_redirect_uri(&root, &id("tiktok")).as_str(),
            "https://h/api/v1/platforms/tiktok/oauth/callback"
        );
        let no_slash = Url::parse("https://h/x").unwrap();
        assert_eq!(
            oauth_redirect_uri(&no_slash, &id("tiktok")).as_str(),
            "https://h/x/api/v1/platforms/tiktok/oauth/callback"
        );
    }
}
