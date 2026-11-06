//! The OpenAI-compatible protocol: `POST {base}/chat/completions` and `GET {base}/models`.
//!
//! Reading is tolerant: unknown fields are ignored, `usage` is optional, `reasoning_content`
//! is ignored, a leading `<think>` block is stripped, and `content` may be a string or an
//! array of text parts. Preset extras (headers, body fields) come from the preset, never from
//! the host name.

use std::fmt;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use postit_http::{HttpError, PublicClient};
use reqwest::Method;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use url::Url;
use zeroize::Zeroizing;

use crate::config::{ProviderConfig, ProviderSecret, TokenParam};
use crate::error::{FailureReason, LlmError};
use crate::presets::Preset;
use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, Finish, LlmProvider, OutputMode, Part, RemoteModel,
    Role, Usage,
};
use crate::send::{extract_json, send, strip_think};

/// A provider speaking the `OpenAI` chat completions protocol.
pub struct OpenAiCompatible {
    client: PublicClient,
    base_url: Url,
    secret: ProviderSecret,
    token_param: TokenParam,
    temperature: Option<f32>,
    extra_headers: &'static [(&'static str, &'static str)],
    extra_body: Option<fn() -> Value>,
}

impl fmt::Debug for OpenAiCompatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatible")
            .field("host", &self.base_url.host_str())
            .field("token_param", &self.token_param)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatible {
    /// A provider at `base_url` (requests go to `{base_url}/chat/completions`). The token
    /// parameter and temperature come from `config`, the query parameters from `secret`; the token parameter
    /// falls back to the preset's, then to `max_tokens`.
    #[must_use]
    pub fn new(
        client: PublicClient,
        base_url: Url,
        secret: ProviderSecret,
        config: &ProviderConfig,
        preset: Option<&'static Preset>,
    ) -> Self {
        let token_param = config
            .options
            .token_param
            .or_else(|| preset.map(|p| p.token_param))
            .unwrap_or(TokenParam::MaxTokens);
        Self {
            client,
            base_url,
            secret,
            token_param,
            temperature: config.options.temperature,
            extra_headers: preset.map_or(&[], |p| p.extra_headers),
            extra_body: preset.and_then(|p| p.extra_body),
        }
    }

    /// `path` under the base, with the configured query parameters appended.
    fn url(&self, path: &str) -> Url {
        self.secret.url(&self.base_url, path)
    }

    /// The preset's headers, then the bearer key, then the configured headers, each later one
    /// replacing an earlier one of the same name. Secret values are marked sensitive.
    fn headers(&self) -> Result<HeaderMap, LlmError> {
        let mut headers = HeaderMap::new();
        for (name, value) in self.extra_headers {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| LlmError::new(FailureReason::BadRequest))?,
                HeaderValue::from_str(value)
                    .map_err(|_| LlmError::new(FailureReason::BadRequest))?,
            );
        }
        if let Some(key) = &self.secret.api_key {
            // The joined string holds the key, so it is wiped when dropped.
            let bearer = Zeroizing::new(format!("Bearer {}", key.expose_secret()));
            let mut value = HeaderValue::from_str(&bearer)
                .map_err(|_| LlmError::new(FailureReason::BadRequest))?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        for (name, value) in &self.secret.headers {
            let name = HeaderName::try_from(name.as_str())
                .map_err(|_| LlmError::new(FailureReason::BadRequest))?;
            let mut value = HeaderValue::from_str(value.expose_secret())
                .map_err(|_| LlmError::new(FailureReason::BadRequest))?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        Ok(headers)
    }

    /// The chat completions body for `request`.
    fn body(&self, request: &ChatRequest) -> Value {
        let mut messages = Vec::with_capacity(request.messages.len() + 1);
        if !request.system.is_empty() {
            messages.push(json!({"role": "system", "content": request.system}));
        }
        messages.extend(request.messages.iter().map(message));
        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(request.model.clone()));
        body.insert("messages".to_owned(), Value::Array(messages));
        let token_field = match self.token_param {
            TokenParam::MaxTokens => "max_tokens",
            TokenParam::MaxCompletionTokens => "max_completion_tokens",
        };
        body.insert(token_field.to_owned(), Value::from(request.max_tokens));
        if let Some(t) = request.temperature.or(self.temperature) {
            body.insert("temperature".to_owned(), temperature(t));
        }
        match &request.output {
            OutputMode::JsonSchema { name, schema } => {
                body.insert(
                    "response_format".to_owned(),
                    json!({"type": "json_schema", "json_schema": {"name": name, "schema": schema}}),
                );
            }
            OutputMode::JsonObject => {
                body.insert("response_format".to_owned(), json!({"type": "json_object"}));
            }
            OutputMode::Text => {}
        }
        if let Some(Value::Object(extra)) = self.extra_body.map(|f| f()) {
            body.extend(extra);
        }
        Value::Object(body)
    }
}

/// Builds one request; `send` calls it once per attempt.
pub(crate) fn build(
    method: Method,
    url: &Url,
    headers: &HeaderMap,
    json_body: Option<&Bytes>,
) -> reqwest::Request {
    let mut request = reqwest::Request::new(method, url.clone());
    *request.headers_mut() = headers.clone();
    if let Some(body) = json_body {
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        *request.body_mut() = Some(reqwest::Body::from(body.clone()));
    }
    request
}

/// A temperature as the decimal the user typed (`0.3`, not `0.30000001192092896`).
fn temperature(t: f32) -> Value {
    t.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

/// One message: a string when it is text only, else an array of text and image parts.
fn message(m: &ChatMessage) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let text_only = m.parts.iter().all(|p| matches!(p, Part::Text(_)));
    let content = if text_only {
        let text: String = m
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                Part::Image { .. } => None,
            })
            .collect();
        Value::String(text)
    } else {
        Value::Array(
            m.parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => json!({"type": "text", "text": t}),
                    Part::Image { jpeg } => json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg))}
                    }),
                })
                .collect(),
        )
    };
    json!({"role": role, "content": content})
}

fn invalid() -> LlmError {
    LlmError::new(FailureReason::InvalidResponse)
}

/// Reads a chat completion. A body that is not JSON, or has no `choices` array, is not the
/// protocol's answer (`invalid_response`). A protocol answer with no usable message (empty
/// `choices`, a choice without `message`) is an answer with no text: the provider was paid, and
/// the drafter repairs it like any unusable answer. Everything else is optional.
fn parse_completion(body: &[u8], output: &OutputMode) -> Result<ChatResponse, LlmError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let first = choices
        .first()
        .and_then(|c| Some((c, c.get("message")?.as_object()?)));
    let Some((choice, message)) = first else {
        return Ok(ChatResponse {
            text: String::new(),
            usage: usage(value.get("usage")),
            finish: Finish::Other,
        });
    };
    let content = match message.get("content") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|p| {
                p.get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|t| t == "text")
            })
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect(),
        Some(_) => return Err(invalid()),
    };
    let refused = message
        .get("refusal")
        .and_then(Value::as_str)
        .is_some_and(|r| !r.is_empty());
    let finish = if refused {
        Finish::Refused
    } else {
        match choice.get("finish_reason").and_then(Value::as_str) {
            Some("stop") => Finish::Stop,
            Some("length") => Finish::Length,
            Some("content_filter") => Finish::Refused,
            _ => Finish::Other,
        }
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

/// Token counts when both are present and fit.
fn usage(value: Option<&Value>) -> Option<Usage> {
    let value = value?;
    let count = |field: &str| {
        value
            .get(field)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Some(Usage {
        input_tokens: count("prompt_tokens")?,
        output_tokens: count("completion_tokens")?,
    })
}

/// Reads a model listing: `data` must be an array; entries without a string `id` are skipped.
fn parse_models(body: &[u8]) -> Result<Vec<RemoteModel>, LlmError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?;
            Some(RemoteModel {
                id: id.to_owned(),
                name: m.get("name").and_then(Value::as_str).map(ToOwned::to_owned),
            })
        })
        .collect())
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = self.url("chat/completions");
        let headers = self.headers()?;
        let body = serde_json::to_vec(&self.body(request))
            .map(Bytes::from)
            .map_err(|_| LlmError::new(FailureReason::BadRequest))?;
        let answer = send(
            &self.client,
            request.deadline,
            self.secret.sensitivity(),
            || Ok::<_, HttpError>(build(Method::POST, &url, &headers, Some(&body))),
        )
        .await?;
        parse_completion(&answer, &request.output)
    }

    async fn models(&self, deadline: Instant) -> Result<Vec<RemoteModel>, LlmError> {
        let url = self.url("models");
        let headers = self.headers()?;
        let answer = send(&self.client, deadline, self.secret.sensitivity(), || {
            Ok::<_, HttpError>(build(Method::GET, &url, &headers, None))
        })
        .await?;
        parse_models(&answer)
    }
}
