//! The opencode-shaped provider configuration: parsing a pasted entry, moving secrets out of
//! it, and the rules about base URLs.
//!
//! postit never reads the server's environment or files for a user: `{env:...}` and
//! `{file:...}` values are refused.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

use postit_core::{Message, Violation};
use postit_http::{AddressPolicy, is_allowed};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use url::{Host, Url};
use zeroize::Zeroize as _;

use crate::presets::Preset;

/// Default output token limit.
const DEFAULT_MAX_TOKENS: u32 = 8_192;

/// The Gemini OpenAI-compatible endpoint, used for `@ai-sdk/google` without a base URL.
const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/openai/";

/// The wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    /// `POST {base}/chat/completions`.
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// `POST {base}/v1/messages`.
    #[serde(rename = "anthropic")]
    Anthropic,
}

/// Which request field carries the output token limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenParam {
    /// `max_tokens`.
    #[serde(rename = "max_tokens")]
    MaxTokens,
    /// `max_completion_tokens`.
    #[serde(rename = "max_completion_tokens")]
    MaxCompletionTokens,
}

/// Connection options. The key and header values live in [`ProviderSecret`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderOptions {
    /// Where requests go, already normalized for the protocol.
    #[serde(rename = "baseURL", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Url>,
    /// Names of the headers whose values are sealed in the secret.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// Names of the query parameters appended to every request; their values are sealed in
    /// the secret, since some providers take the key as one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<String>,
    /// Request timeout in milliseconds (opencode's field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Overrides the preset's token limit field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_param: Option<TokenParam>,
    /// Sent only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// A model's token limits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimit {
    /// Context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u32>,
    /// Output limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u32>,
}

/// What a model accepts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modalities {
    /// Input kinds, e.g. `text`, `image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
}

/// One model entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Token limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<ModelLimit>,
    /// What the model accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Modalities>,
}

/// A provider row's `config`: an opencode `provider` entry without secrets, plus postit's
/// `protocol`, `default_model`, and `structured_output`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// The AI SDK package opencode names; maps to a protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The protocol; wins over `npm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
    /// Connection options.
    #[serde(default)]
    pub options: ProviderOptions,
    /// Known models.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, ModelConfig>,
    /// The model used when a request names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// `json_schema`, `json_object`, or `text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<String>,
}

/// The sealed part of a provider: the API key, the header values, and the query values.
#[derive(Default)]
pub struct ProviderSecret {
    /// The API key.
    pub api_key: Option<SecretString>,
    /// Header values by header name.
    pub headers: BTreeMap<String, SecretString>,
    /// Query parameter values by parameter name.
    pub query: BTreeMap<String, SecretString>,
}

impl fmt::Debug for ProviderSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderSecret")
            .field("has_api_key", &self.api_key.is_some())
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("query", &self.query.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderSecret {
    /// The JSON the vault seals.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let open = |map: &BTreeMap<String, SecretString>| -> Map<String, Value> {
            map.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.expose_secret().to_owned())))
                .collect()
        };
        json!({
            "api_key": self.api_key.as_ref().map(|k| k.expose_secret().to_owned()),
            "headers": open(&self.headers),
            "query": open(&self.query),
        })
    }

    /// `path` under `base`, with the sealed query parameters appended.
    #[must_use]
    pub fn url(&self, base: &Url, path: &str) -> Url {
        let mut url = crate::send::join(base, path);
        if !self.query.is_empty() {
            url.query_pairs_mut().extend_pairs(
                self.query
                    .iter()
                    .map(|(name, value)| (name, value.expose_secret())),
            );
        }
        url
    }

    /// How a request URL may be logged: not at all when it carries sealed query values.
    #[must_use]
    pub fn sensitivity(&self) -> postit_http::Sensitivity {
        if self.query.is_empty() {
            postit_http::Sensitivity::Normal
        } else {
            postit_http::Sensitivity::Opaque
        }
    }

    /// Seals the secret: the plaintext JSON the vault needs is built, handed to `seal`, and
    /// wiped, so the key is not left in a buffer nobody zeroes.
    pub fn sealed_with<T>(&self, seal: impl FnOnce(&Value) -> T) -> T {
        let mut value = self.to_value();
        let out = seal(&value);
        wipe(&mut value);
        out
    }

    /// Reads what [`to_value`](Self::to_value) wrote and wipes `value`; `None` when the shape
    /// is wrong.
    #[must_use]
    pub fn from_opened(mut value: Value) -> Option<Self> {
        let secret = Self::from_value(&value);
        wipe(&mut value);
        secret
    }

    /// Reads what [`to_value`](Self::to_value) wrote; `None` when the shape is wrong.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let obj = value.as_object()?;
        let api_key = match obj.get("api_key") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(SecretString::from(s.clone())),
            Some(_) => return None,
        };
        let read = |key: &str| -> Option<BTreeMap<String, SecretString>> {
            let mut out = BTreeMap::new();
            if let Some(map) = obj.get(key) {
                for (k, v) in map.as_object()? {
                    out.insert(k.clone(), SecretString::from(v.as_str()?.to_owned()));
                }
            }
            Some(out)
        };
        Some(Self {
            api_key,
            headers: read("headers")?,
            query: read("query")?,
        })
    }
}

fn msg(key: &'static str, en: &'static str) -> Message {
    Message::new_static(key, en)
}

fn violation(path: &str, key: &'static str, en: &'static str) -> Violation {
    Violation::agent_rule(msg(key, en)).at(path)
}

/// The protocol an `npm` package name implies.
fn protocol_from_npm(npm: &str) -> Option<Protocol> {
    match npm {
        "@ai-sdk/openai-compatible" | "@ai-sdk/openai" | "@ai-sdk/google" => {
            Some(Protocol::OpenAiCompatible)
        }
        "@ai-sdk/anthropic" => Some(Protocol::Anthropic),
        _ => None,
    }
}

impl ProviderConfig {
    /// The protocol: the explicit one, else the one `npm` implies.
    ///
    /// # Errors
    /// A violation at `npm` when neither names a known protocol.
    pub fn protocol(&self) -> Result<Protocol, Violation> {
        if let Some(p) = self.protocol {
            return Ok(p);
        }
        self.npm
            .as_deref()
            .and_then(protocol_from_npm)
            .ok_or_else(|| {
                violation(
                    "npm",
                    "ai.protocol_unknown",
                    "Set \"protocol\" to \"openai-compatible\" or \"anthropic\"; this package is not known.",
                )
            })
    }

    /// Whether `model` takes images: its `modalities.input` when listed, else the preset's
    /// default.
    #[must_use]
    pub fn takes_images(&self, model: &str, preset: Option<&Preset>) -> bool {
        let listed = self
            .models
            .get(model)
            .and_then(|m| m.modalities.as_ref())
            .and_then(|m| m.input.as_ref());
        match listed {
            Some(input) => input.iter().any(|k| k.eq_ignore_ascii_case("image")),
            None => preset.is_some_and(|p| p.images_by_default),
        }
    }

    /// The output token limit: 8,192, or the model's `limit.output` when that is lower.
    #[must_use]
    pub fn max_tokens(&self, model: &str) -> u32 {
        let limit = self
            .models
            .get(model)
            .and_then(|m| m.limit.as_ref())
            .and_then(|l| l.output)
            .filter(|n| *n > 0);
        limit.map_or(DEFAULT_MAX_TOKENS, |n| n.min(DEFAULT_MAX_TOKENS))
    }
}

/// Strips what the protocol adds itself: the Anthropic API adds `/v1/...`, so a base ending
/// in `/v1` loses that segment. OpenAI-compatible bases are kept as typed.
#[must_use]
pub fn normalize_base(protocol: Protocol, mut url: Url) -> Url {
    if protocol == Protocol::Anthropic {
        let path = url.path().trim_end_matches('/');
        let stripped = path.strip_suffix("/v1").unwrap_or(path);
        let path = if stripped.is_empty() { "/" } else { stripped };
        let path = path.to_owned();
        url.set_path(&path);
    }
    url
}

/// Checks a base URL: http or https, no credentials, https unless the host is private, and a
/// private host only when `allow_private`. The address policy still applies at send time.
///
/// # Errors
/// A violation at `options.baseURL`.
pub fn validate_base_url(url: &Url, allow_private: bool) -> Result<(), Violation> {
    const PATH: &str = "options.baseURL";
    if !matches!(url.scheme(), "http" | "https") {
        return Err(violation(
            PATH,
            "ai.base_url_https_required",
            "The address must start with https://.",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(violation(
            PATH,
            "ai.base_url_credentials",
            "Put the key in the key field, not in the address.",
        ));
    }
    // A key passed in the query would be stored in the clear with the address.
    if url.query().is_some() {
        return Err(violation(
            PATH,
            "ai.base_url_query",
            "Put query parameters under options.query, not in the address.",
        ));
    }
    let private = match url.host() {
        None => {
            return Err(violation(
                PATH,
                "ai.base_url_invalid",
                "The address has no host.",
            ));
        }
        Some(Host::Domain(d)) => {
            // A fully qualified `localhost.` is the same host.
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => private_ip(IpAddr::V4(ip))?,
        Some(Host::Ipv6(ip)) => private_ip(IpAddr::V6(ip))?,
    };
    if private {
        if allow_private {
            return Ok(());
        }
        return Err(violation(
            PATH,
            "ai.private_endpoint_refused",
            "Local and private addresses need ai.allow_private_endpoints turned on by the administrator.",
        ));
    }
    if url.scheme() != "https" {
        return Err(violation(
            PATH,
            "ai.base_url_https_required",
            "The address must start with https://.",
        ));
    }
    Ok(())
}

/// Whether `ip` is a private address (`true`) or a public one (`false`). One that no policy
/// ever allows (unspecified, multicast, broadcast, reserved) is refused outright, since
/// turning on `ai.allow_private_endpoints` would not make it reachable.
fn private_ip(ip: IpAddr) -> Result<bool, Violation> {
    if !is_allowed(
        ip,
        AddressPolicy {
            allow_private: true,
        },
    ) {
        return Err(violation(
            "options.baseURL",
            "ai.base_url_forbidden",
            "This address can never be reached. Give the address of the AI service.",
        ));
    }
    Ok(!is_allowed(ip, AddressPolicy::strict()))
}

/// Zeroes every string in `value`, for plaintext JSON that held a secret.
fn wipe(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(map) => map.values_mut().for_each(wipe),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn has_substitution(s: &str) -> bool {
    s.contains("{env:") || s.contains("{file:")
}

fn substitution_violation(path: &str) -> Violation {
    violation(
        path,
        "ai.substitution_unsupported",
        "Values like {env:NAME} and {file:path} are not supported. Paste the value itself.",
    )
}

/// What the entry's `options` held that must not be stored in the config.
struct Extracted {
    api_key: Option<SecretString>,
    headers: BTreeMap<String, SecretString>,
    query: BTreeMap<String, SecretString>,
}

/// Moves the string values of `options.<field>` into `out`, leaving the names, and refuses
/// substitutions and values that are not strings.
fn extract_values(
    options: &mut Map<String, Value>,
    field: &str,
    out: &mut BTreeMap<String, SecretString>,
    violations: &mut Vec<Violation>,
) {
    let shape = |path: &str| {
        violation(
            path,
            "ai.config_invalid",
            "The provider settings are not in the expected shape.",
        )
    };
    let mut names = Vec::new();
    match options.remove(field) {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => {
            for (name, value) in map {
                let path = format!("options.{field}.{name}");
                match value {
                    Value::String(s) if has_substitution(&s) => {
                        violations.push(substitution_violation(&path));
                    }
                    Value::String(s) => {
                        out.insert(name.clone(), SecretString::from(s));
                        names.push(Value::String(name));
                    }
                    _ => violations.push(shape(&path)),
                }
            }
        }
        Some(_) => violations.push(shape(&format!("options.{field}"))),
    }
    options.insert(field.to_owned(), Value::Array(names));
}

/// Removes `options.apiKey` and the `options.headers` and `options.query` values from
/// `entry`, leaving the names, and refuses substitutions.
fn extract_secrets(entry: &mut Map<String, Value>, violations: &mut Vec<Violation>) -> Extracted {
    let mut out = Extracted {
        api_key: None,
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
    };
    let Some(options) = entry.get_mut("options") else {
        return out;
    };
    let Some(options) = options.as_object_mut() else {
        violations.push(violation(
            "options",
            "ai.config_invalid",
            "The provider settings are not in the expected shape.",
        ));
        return out;
    };
    match options.remove("apiKey") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if has_substitution(&s) => {
            violations.push(substitution_violation("options.apiKey"));
        }
        Some(Value::String(s)) => out.api_key = Some(SecretString::from(s)),
        Some(_) => violations.push(violation(
            "options.apiKey",
            "ai.config_invalid",
            "The provider settings are not in the expected shape.",
        )),
    }
    extract_values(options, "headers", &mut out.headers, violations);
    extract_values(options, "query", &mut out.query, violations);
    out
}

/// Picks the entry out of a paste: the entry itself, or the one inside `{ "provider": {...} }`.
fn entry_of(raw: &Value) -> Result<(Map<String, Value>, Option<String>), Violation> {
    let shape = || {
        violation(
            "",
            "ai.config_invalid",
            "The provider settings are not in the expected shape.",
        )
    };
    let obj = raw.as_object().ok_or_else(shape)?;
    let hint = obj
        .get("model")
        .and_then(Value::as_str)
        .map(|m| m.split_once('/').map_or(m, |(_, model)| model).to_owned())
        .filter(|m| !m.is_empty());
    match obj.get("provider").and_then(Value::as_object) {
        Some(map) => {
            let mut entries = map.values();
            match (entries.next(), entries.next()) {
                (None, _) => Err(violation(
                    "provider",
                    "ai.provider_empty",
                    "The provider entry is empty.",
                )),
                (Some(Value::Object(entry)), None) => Ok((entry.clone(), hint)),
                (Some(_), None) => Err(shape()),
                _ => Err(violation(
                    "provider",
                    "ai.provider_ambiguous",
                    "Paste one provider at a time.",
                )),
            }
        }
        None => Ok((obj.clone(), hint)),
    }
}

/// Reads a pasted opencode `provider` entry (optionally wrapped as `{ "provider": { "<id>":
/// {...} } }`), moves the API key and header values into the secret, resolves the protocol,
/// and checks and normalizes the base URL. Nothing is read from the environment.
///
/// # Errors
/// Every violation found, each at the path of the offending field.
pub fn parse_pasted(
    raw: &Value,
    allow_private: bool,
) -> Result<(ProviderConfig, ProviderSecret), Vec<Violation>> {
    let (mut entry, hint) = entry_of(raw).map_err(|v| vec![v])?;
    let mut violations = Vec::new();
    let secrets = extract_secrets(&mut entry, &mut violations);
    // A base URL that is a substitution or not an address is reported at its own path and
    // left out, so the rest of the entry is still read and checked.
    let base_refused = match entry.get("options").and_then(|o| o.get("baseURL")) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if has_substitution(s) => {
            Some(substitution_violation("options.baseURL"))
        }
        Some(Value::String(s)) if Url::parse(s).is_ok() => None,
        Some(_) => Some(violation(
            "options.baseURL",
            "ai.base_url_invalid",
            "This is not a web address. Give the full address, starting with https://.",
        )),
    };
    let base_refused = base_refused.is_some_and(|v| {
        violations.push(v);
        if let Some(o) = entry.get_mut("options").and_then(Value::as_object_mut) {
            o.remove("baseURL");
        }
        true
    });
    let Ok(mut config) = serde_json::from_value::<ProviderConfig>(Value::Object(entry)) else {
        violations.push(violation(
            "",
            "ai.config_invalid",
            "The provider settings are not in the expected shape.",
        ));
        return Err(violations);
    };
    finish(
        &mut config,
        hint,
        allow_private,
        base_refused,
        &mut violations,
    );
    if violations.is_empty() {
        Ok((
            config,
            ProviderSecret {
                api_key: secrets.api_key,
                headers: secrets.headers,
                query: secrets.query,
            },
        ))
    } else {
        Err(violations)
    }
}

/// Resolves the protocol, defaults and checks the base URL, and applies the model hint.
fn finish(
    config: &mut ProviderConfig,
    hint: Option<String>,
    allow_private: bool,
    base_refused: bool,
    violations: &mut Vec<Violation>,
) {
    let protocol = match config.protocol() {
        Ok(p) => {
            config.protocol = Some(p);
            Some(p)
        }
        Err(v) => {
            violations.push(v);
            None
        }
    };
    if config.options.base_url.is_none()
        && config.npm.as_deref() == Some("@ai-sdk/google")
        && let Ok(url) = Url::parse(GEMINI_BASE)
    {
        config.options.base_url = Some(url);
    }
    match (config.options.base_url.take(), protocol) {
        (Some(url), Some(protocol)) => match validate_base_url(&url, allow_private) {
            Ok(()) => config.options.base_url = Some(normalize_base(protocol, url)),
            Err(v) => violations.push(v),
        },
        (Some(url), None) => config.options.base_url = Some(url),
        (None, _) if base_refused => {}
        (None, _) => violations.push(violation(
            "options.baseURL",
            "ai.base_url_required",
            "The provider needs a base address.",
        )),
    }
    if let Some(mode) = config.structured_output.as_deref()
        && !matches!(mode, "json_schema" | "json_object" | "text")
    {
        violations.push(violation(
            "structured_output",
            "ai.structured_output_invalid",
            "Use json_schema, json_object, or text.",
        ));
    }
    if config.default_model.is_none() {
        config.default_model = hint;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use secrecy::ExposeSecret;
    use serde_json::{Value, json};
    use url::Url;

    use super::*;
    use crate::presets::preset;

    fn paste(v: &Value) -> (ProviderConfig, ProviderSecret) {
        parse_pasted(v, false).unwrap()
    }

    #[test]
    fn npm_maps_to_protocol() {
        for (npm, want) in [
            ("@ai-sdk/openai-compatible", Protocol::OpenAiCompatible),
            ("@ai-sdk/openai", Protocol::OpenAiCompatible),
            ("@ai-sdk/anthropic", Protocol::Anthropic),
            ("@ai-sdk/google", Protocol::OpenAiCompatible),
        ] {
            let (cfg, _) =
                paste(&json!({"npm": npm, "options": {"baseURL": "https://api.example.com/v1"}}));
            assert_eq!(cfg.protocol().unwrap(), want, "{npm}");
        }
        let (cfg, _) = paste(&json!({"npm": "@ai-sdk/google"}));
        assert_eq!(
            cfg.options.base_url.unwrap().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/openai/"
        );
        let (cfg, _) = paste(
            &json!({"npm": "@ai-sdk/google", "options": {"baseURL": "https://proxy.example.com/g/"}}),
        );
        assert_eq!(
            cfg.options.base_url.unwrap().as_str(),
            "https://proxy.example.com/g/"
        );
    }

    #[test]
    fn explicit_protocol_wins() {
        let (cfg, _) = paste(&json!({
            "npm": "@ai-sdk/anthropic",
            "protocol": "openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1"}
        }));
        assert_eq!(cfg.protocol().unwrap(), Protocol::OpenAiCompatible);
        let (cfg, _) = paste(&json!({
            "protocol": "anthropic",
            "options": {"baseURL": "https://api.example.com"}
        }));
        assert_eq!(cfg.protocol().unwrap(), Protocol::Anthropic);
    }

    #[test]
    fn unknown_npm_without_protocol_is_violation() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/mistral", "options": {"baseURL": "https://a.example.com"}}),
            false,
        )
        .unwrap_err();
        assert!(
            err.iter().any(|v| v.path.as_deref() == Some("npm")),
            "{err:?}"
        );
    }

    #[test]
    fn substitutions_in_query_values_refused() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai-compatible", "options": {
                "baseURL": "https://api.example.com/v1",
                "query": {"api-version": "2026-01-01", "key": "{env:AZURE_KEY}", "f": "{file:~/k}"}
            }}),
            false,
        )
        .unwrap_err();
        let paths: Vec<_> = err.iter().filter_map(|v| v.path.as_deref()).collect();
        assert_eq!(paths, ["options.query.f", "options.query.key"], "{err:?}");
        assert!(
            err.iter()
                .all(|v| v.params["message"]["key"] == "ai.substitution_unsupported")
        );
    }

    #[test]
    fn env_and_file_substitutions_refused() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "https://api.openai.com/v1", "apiKey": "{env:OPENAI_API_KEY}"}}),
            false,
        )
        .unwrap_err();
        let v = err
            .iter()
            .find(|v| v.path.as_deref() == Some("options.apiKey"))
            .unwrap();
        assert_eq!(v.params["message"]["key"], "ai.substitution_unsupported");

        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "https://api.openai.com/v1", "headers": {"Authorization": "Bearer {file:~/k}"}}}),
            false,
        )
        .unwrap_err();
        let v = err
            .iter()
            .find(|v| v.path.as_deref() == Some("options.headers.Authorization"))
            .unwrap();
        assert_eq!(v.params["message"]["key"], "ai.substitution_unsupported");
    }

    #[test]
    fn api_key_and_headers_move_to_secret() {
        let (cfg, secret) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "Mine",
            "options": {
                "baseURL": "https://api.example.com/v1",
                "apiKey": "sk-very-secret",
                "headers": {"Authorization": "Bearer hdr-secret"}
            },
            "models": {"m": {"name": "M", "limit": {"context": 1000, "output": 500}}}
        }));
        assert_eq!(cfg.options.headers, vec!["Authorization".to_owned()]);
        let ser = serde_json::to_string(&cfg).unwrap();
        assert!(!ser.contains("sk-very-secret"), "{ser}");
        assert!(!ser.contains("hdr-secret"), "{ser}");
        assert!(ser.contains("baseURL"), "{ser}");
        assert_eq!(
            secret.api_key.as_ref().unwrap().expose_secret(),
            "sk-very-secret"
        );
        assert_eq!(
            secret.headers["Authorization"].expose_secret(),
            "Bearer hdr-secret"
        );
        // Sealed round trip.
        let back = ProviderSecret::from_value(&secret.to_value()).unwrap();
        assert_eq!(back.api_key.unwrap().expose_secret(), "sk-very-secret");
        assert_eq!(
            back.headers["Authorization"].expose_secret(),
            "Bearer hdr-secret"
        );
        // The stored config round trips.
        let again: ProviderConfig = serde_json::from_str(&ser).unwrap();
        assert_eq!(again.models["m"].limit.as_ref().unwrap().output, Some(500));
    }

    #[test]
    fn query_values_move_to_secret() {
        let (cfg, secret) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1", "query": {"key": "AIza-secret"}}
        }));
        assert_eq!(cfg.options.query, ["key"]);
        let ser = serde_json::to_string(&cfg).unwrap();
        assert!(!ser.contains("AIza-secret"), "{ser}");
        assert!(!format!("{secret:?}").contains("AIza-secret"));
        let back = ProviderSecret::from_value(&secret.to_value()).unwrap();
        let base = Url::parse("https://api.example.com/v1").unwrap();
        assert_eq!(
            back.url(&base, "chat/completions").as_str(),
            "https://api.example.com/v1/chat/completions?key=AIza-secret"
        );
        assert_eq!(back.sensitivity(), postit_http::Sensitivity::Opaque);
        assert_eq!(
            ProviderSecret::default().sensitivity(),
            postit_http::Sensitivity::Normal
        );
    }

    #[test]
    fn base_url_with_query_refused() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai-compatible", "options": {"baseURL": "https://h.example/v1?api-key=AIza-secret"}}),
            false,
        )
        .unwrap_err();
        assert!(
            err.iter()
                .any(|v| v.params["message"]["key"] == "ai.base_url_query"),
            "{err:?}"
        );
    }

    #[test]
    fn wrapped_provider_entry_accepted() {
        let (cfg, _) = paste(&json!({
            "provider": {"mine": {"npm": "@ai-sdk/openai-compatible", "options": {"baseURL": "https://api.example.com/v1"}, "models": {"a": {}, "b": {}}}},
            "model": "mine/b"
        }));
        assert_eq!(cfg.default_model.as_deref(), Some("b"));
        assert_eq!(cfg.models.len(), 2);
        // An explicit default wins over the hint.
        let (cfg, _) = paste(&json!({
            "provider": {"mine": {"npm": "@ai-sdk/openai-compatible", "default_model": "a", "options": {"baseURL": "https://api.example.com/v1"}}},
            "model": "mine/b"
        }));
        assert_eq!(cfg.default_model.as_deref(), Some("a"));
        // Several providers in one paste are ambiguous.
        let err = parse_pasted(&json!({"provider": {}}), false).unwrap_err();
        assert_eq!(err[0].params["message"]["key"], "ai.provider_empty");
        let err = parse_pasted(&json!({"provider": {"a": {}, "b": {}}}), false).unwrap_err();
        assert!(err.iter().any(|v| v.path.as_deref() == Some("provider")));
    }

    #[test]
    fn https_required_for_public_hosts() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "http://api.example.com/v1"}}),
            true,
        )
        .unwrap_err();
        assert!(
            err.iter()
                .any(|v| v.path.as_deref() == Some("options.baseURL"))
        );
        // A private host over http is fine when allowed.
        parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "http://localhost:11434/v1"}}),
            true,
        )
        .unwrap();
        parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "http://192.168.1.5:8000/v1"}}),
            true,
        )
        .unwrap();
        // No base URL at all for a non-Google npm.
        let err = parse_pasted(&json!({"npm": "@ai-sdk/openai"}), false).unwrap_err();
        assert!(
            err.iter()
                .any(|v| v.path.as_deref() == Some("options.baseURL"))
        );
    }

    #[test]
    fn credentials_in_url_refused() {
        let err = parse_pasted(
            &json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "https://user:pw@api.example.com/v1"}}),
            false,
        )
        .unwrap_err();
        let v = err
            .iter()
            .find(|v| v.path.as_deref() == Some("options.baseURL"))
            .unwrap();
        assert_eq!(v.params["message"]["key"], "ai.base_url_credentials");
        assert!(!format!("{err:?}").contains("pw@"));
    }

    #[test]
    fn private_host_refused_unless_allowed() {
        for url in [
            "http://localhost:11434/v1",
            "https://127.0.0.1/v1",
            "https://10.0.0.4/v1",
            "https://[::1]/v1",
            "http://localhost.:11434/v1",
            "http://LOCALHOST./v1",
            "http://models.localhost./v1",
        ] {
            let raw = json!({"npm": "@ai-sdk/openai-compatible", "options": {"baseURL": url}});
            let err = parse_pasted(&raw, false).unwrap_err();
            let v = err
                .iter()
                .find(|v| v.path.as_deref() == Some("options.baseURL"))
                .unwrap();
            assert_eq!(
                v.params["message"]["key"], "ai.private_endpoint_refused",
                "{url}"
            );
            parse_pasted(&raw, true).unwrap();
        }
    }

    /// Unspecified, multicast, broadcast, and reserved addresses are never reachable, so they
    /// are refused even with private endpoints allowed, not offered as "private".
    #[test]
    fn always_forbidden_addresses_refused() {
        for url in [
            "http://0.0.0.0/v1",
            "http://224.0.0.1/v1",
            "http://255.255.255.255/v1",
            "http://240.0.0.1/v1",
            "http://[::]/v1",
            "http://[ff02::1]/v1",
        ] {
            let raw = json!({"npm": "@ai-sdk/openai-compatible", "options": {"baseURL": url}});
            for allow_private in [false, true] {
                let err = parse_pasted(&raw, allow_private).unwrap_err();
                let v = err
                    .iter()
                    .find(|v| v.path.as_deref() == Some("options.baseURL"))
                    .unwrap();
                assert_eq!(
                    v.params["message"]["key"], "ai.base_url_forbidden",
                    "{url} {allow_private}"
                );
            }
        }
    }

    /// A base URL that is not an address is reported at its own path, and the rest of the
    /// entry is still checked.
    #[test]
    fn malformed_base_url_reported_at_its_path() {
        for bad in [json!("not a url"), json!("https://"), json!(42)] {
            let raw = json!({"npm": "@ai-sdk/mistral", "options": {"baseURL": bad}});
            let err = parse_pasted(&raw, false).unwrap_err();
            let paths: Vec<_> = err.iter().filter_map(|v| v.path.as_deref()).collect();
            assert!(paths.contains(&"options.baseURL"), "{bad}: {paths:?}");
            assert!(paths.contains(&"npm"), "{bad}: {paths:?}");
            assert!(!paths.contains(&""), "{bad}: {paths:?}");
            let v = err
                .iter()
                .find(|v| v.path.as_deref() == Some("options.baseURL"))
                .unwrap();
            assert_eq!(v.params["message"]["key"], "ai.base_url_invalid", "{bad}");
        }
    }

    #[test]
    fn anthropic_base_drops_version_segment() {
        let want = "https://api.anthropic.com/";
        for u in [
            "https://api.anthropic.com",
            "https://api.anthropic.com/",
            "https://api.anthropic.com/v1",
            "https://api.anthropic.com/v1/",
        ] {
            let url = normalize_base(Protocol::Anthropic, Url::parse(u).unwrap());
            assert_eq!(url.as_str(), want, "{u}");
        }
        let url = normalize_base(
            Protocol::Anthropic,
            Url::parse("https://proxy.example.com/anthropic/v1").unwrap(),
        );
        assert_eq!(url.as_str(), "https://proxy.example.com/anthropic");
        for u in ["https://api.openai.com/v1", "https://api.openai.com/v1/"] {
            let url = normalize_base(Protocol::OpenAiCompatible, Url::parse(u).unwrap());
            assert_eq!(url.as_str(), u);
        }
        // Applied on paste.
        let (cfg, _) = paste(
            &json!({"npm": "@ai-sdk/anthropic", "options": {"baseURL": "https://api.anthropic.com/v1"}}),
        );
        assert_eq!(cfg.options.base_url.unwrap().as_str(), want);
    }

    #[test]
    fn modalities_decide_images() {
        let (cfg, _) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1"},
            "models": {
                "vision": {"modalities": {"input": ["text", "image"]}},
                "texty": {"modalities": {"input": ["text"]}},
                "plain": {}
            }
        }));
        let on = preset("openai");
        let off = preset("ollama");
        assert!(cfg.takes_images("vision", off));
        assert!(!cfg.takes_images("texty", on));
        assert!(cfg.takes_images("plain", on));
        assert!(!cfg.takes_images("plain", off));
        assert!(cfg.takes_images("unlisted", on));
        assert!(!cfg.takes_images("unlisted", None));
    }

    #[test]
    fn max_tokens_capped_by_model_limit() {
        let (cfg, _) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1"},
            "models": {"small": {"limit": {"context": 8000, "output": 4096}}, "big": {"limit": {"output": 100_000}}, "none": {}}
        }));
        assert_eq!(cfg.max_tokens("none"), 8_192);
        assert_eq!(cfg.max_tokens("small"), 4_096);
        assert_eq!(cfg.max_tokens("big"), 8_192);
        assert_eq!(cfg.max_tokens("unlisted"), 8_192);
    }

    #[test]
    fn postit_additions_parse() {
        let (cfg, _) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1", "query": {"api-version": "2026-01-01"}, "timeout": 60000, "token_param": "max_completion_tokens", "temperature": 0.5},
            "structured_output": "json_schema"
        }));
        assert_eq!(cfg.options.query, ["api-version"]);
        assert_eq!(cfg.options.timeout, Some(60_000));
        assert_eq!(
            cfg.options.token_param,
            Some(TokenParam::MaxCompletionTokens)
        );
        assert_eq!(cfg.options.temperature, Some(0.5));
        assert_eq!(cfg.structured_output.as_deref(), Some("json_schema"));
    }

    #[test]
    fn secret_debug_redacted() {
        let (_, secret) = paste(&json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": "https://api.example.com/v1", "apiKey": "sk-very-secret", "headers": {"X-Org": "org-secret"}}
        }));
        let dbg = format!("{secret:?}");
        assert!(!dbg.contains("sk-very-secret"), "{dbg}");
        assert!(!dbg.contains("org-secret"), "{dbg}");
    }
}
