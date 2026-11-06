//! The native Anthropic protocol: `POST {base}/v1/messages` and `GET {base}/v1/models`.
//!
//! The base URL is the host (and any proxy prefix) without `/v1`: `normalize_base` strips a
//! trailing `/v1`, and this module adds `v1/...` itself.

use std::fmt;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use postit_http::{HttpError, PublicClient};
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tokio::time::Instant;
use url::Url;

use crate::config::ProviderSecret;
use crate::error::{FailureReason, LlmError};
use crate::openai::build;
use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, Finish, LlmProvider, OutputMode, Part, RemoteModel,
    Role, Usage,
};
use crate::send::{extract_json, send, strip_think};

/// The `anthropic-version` header value.
///
/// Source: <https://platform.claude.com/docs/en/api/versioning>, read 2026-11-06. It lists
/// `2026-10-21` as the latest version ("Anthropic recommends using the latest API version")
/// and `2023-06-01` as the previous one. This version removes the `temperature`, `top_p`, and
/// `top_k` request parameters: a request that sends any of them gets a 400 on every model, so
/// this provider never sends `temperature`, whatever the config says. Everything else the
/// provider uses (`system`, image blocks, `output_config.format`, `/v1/models` paging) is
/// unchanged between the two versions.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";

/// The most model pages read in one call.
const MAX_MODEL_PAGES: usize = 10;

/// A provider speaking the native Anthropic Messages protocol.
pub struct Anthropic {
    client: PublicClient,
    base_url: Url,
    secret: ProviderSecret,
}

impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("host", &self.base_url.host_str())
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl Anthropic {
    /// A provider at `base_url` (requests go to `{base_url}/v1/messages`; pass the base through
    /// `normalize_base` first).
    #[must_use]
    pub fn new(client: PublicClient, base_url: Url, secret: ProviderSecret) -> Self {
        Self {
            client,
            base_url,
            secret,
        }
    }

    fn url(&self, path: &str) -> Url {
        self.secret.url(&self.base_url, path)
    }

    /// The version header, the key, then the configured headers (a later one replaces an
    /// earlier one of the same name). Secret values are marked sensitive.
    fn headers(&self) -> Result<HeaderMap, LlmError> {
        let bad = || LlmError::new(FailureReason::BadRequest);
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(key) = &self.secret.api_key {
            let mut value = HeaderValue::from_str(key.expose_secret()).map_err(|_| bad())?;
            value.set_sensitive(true);
            headers.insert("x-api-key", value);
        }
        for (name, value) in &self.secret.headers {
            let name = HeaderName::try_from(name.as_str()).map_err(|_| bad())?;
            let mut value = HeaderValue::from_str(value.expose_secret()).map_err(|_| bad())?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        Ok(headers)
    }
}

/// The messages body for `request`. `temperature` is deliberately never sent (see
/// [`ANTHROPIC_VERSION`]).
fn body(request: &ChatRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "messages": request.messages.iter().map(message).collect::<Vec<_>>(),
    });
    if !request.system.is_empty() {
        body["system"] = Value::String(request.system.clone());
    }
    if let OutputMode::JsonSchema { schema, .. } = &request.output {
        body["output_config"] = json!({"format": {"type": "json_schema", "schema": schema}});
    }
    body
}

fn message(m: &ChatMessage) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = m
        .parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => json!({"type": "text", "text": t}),
            Part::Image { jpeg } => json!({
                "type": "image",
                "source": {"type": "base64", "media_type": "image/jpeg", "data": STANDARD.encode(jpeg)}
            }),
        })
        .collect();
    json!({"role": role, "content": content})
}

fn invalid() -> LlmError {
    LlmError::new(FailureReason::InvalidResponse)
}

/// Reads a message: `content` must be an array; its `text` blocks are joined, other blocks
/// (thinking and the like) are ignored.
fn parse_message(body: &[u8], output: &OutputMode) -> Result<ChatResponse, LlmError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let content: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    let finish = match value.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn" | "stop_sequence") => Finish::Stop,
        Some("max_tokens") => Finish::Length,
        Some("refusal") => Finish::Refused,
        _ => Finish::Other,
    };
    let answer = strip_think(&content);
    let text = match output {
        OutputMode::JsonSchema { .. } | OutputMode::JsonObject => {
            extract_json(answer).unwrap_or(answer)
        }
        OutputMode::Text => answer,
    };
    Ok(ChatResponse {
        text: text.to_owned(),
        usage: usage(value.get("usage")),
        finish,
    })
}

fn usage(value: Option<&Value>) -> Option<Usage> {
    let value = value?;
    let count = |field: &str| {
        value
            .get(field)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Some(Usage {
        input_tokens: count("input_tokens")?,
        output_tokens: count("output_tokens")?,
    })
}

/// One page of models: the models, and the cursor for the next page when there is one.
fn parse_models_page(body: &[u8]) -> Result<(Vec<RemoteModel>, Option<String>), LlmError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let models: Vec<RemoteModel> = data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?;
            Some(RemoteModel {
                id: id.to_owned(),
                name: m
                    .get("display_name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            })
        })
        .collect();
    let has_more = value
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let next = if has_more {
        value
            .get("last_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| models.last().map(|m| m.id.clone()))
    } else {
        None
    };
    Ok((models, next))
}

#[async_trait]
impl LlmProvider for Anthropic {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = self.url("v1/messages");
        let headers = self.headers()?;
        let payload = serde_json::to_vec(&body(request))
            .map(Bytes::from)
            .map_err(|_| LlmError::new(FailureReason::BadRequest))?;
        let answer = send(
            &self.client,
            request.deadline,
            self.secret.sensitivity(),
            || Ok::<_, HttpError>(build(Method::POST, &url, &headers, Some(&payload))),
        )
        .await?;
        parse_message(&answer, &request.output)
    }

    async fn models(&self, deadline: Instant) -> Result<Vec<RemoteModel>, LlmError> {
        let headers = self.headers()?;
        let mut models = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_MODEL_PAGES {
            let mut url = self.url("v1/models");
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("limit", "1000");
                if let Some(after) = &after {
                    pairs.append_pair("after_id", after);
                }
            }
            let answer = send(&self.client, deadline, self.secret.sensitivity(), || {
                Ok::<_, HttpError>(build(Method::GET, &url, &headers, None))
            })
            .await?;
            let (page, next) = parse_models_page(&answer)?;
            models.extend(page);
            match next {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }
        Ok(models)
    }
}
