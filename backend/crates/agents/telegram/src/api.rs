//! The Bot API client: every call, and all error mapping.
//!
//! The bot token sits in the URL path of every call. It is exposed only to build that URL,
//! and the request goes out only through [`send_with_retry`] with [`Sensitivity::Opaque`], so no
//! span, log, or error shows the URL. Nothing here formats a `reqwest::Error`, whose text can
//! carry the URL.

use std::sync::Arc;
use std::time::Duration;

use postit_agent_kit::{ErrorHook, Phase, classify};
use postit_core::{AgentError, Clock};
use postit_http::{
    HttpError, Idempotency, Jitter, RateLimitHeaders, RetryPolicy, RetryRequest, Sensitivity,
    send_with_retry,
};
use reqwest::multipart::Form;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use crate::credentials::ChatRef;

/// Why a Bot API call did not return a result.
#[derive(Debug)]
pub(crate) enum ApiError {
    /// An error the engine understands.
    Agent(AgentError),
    /// The group became a supergroup; the chat has this ID now. The call did nothing.
    Migrated(i64),
    /// Another program owns the bot's updates (a webhook or a long poll): HTTP 409.
    Conflict,
}

impl From<AgentError> for ApiError {
    fn from(err: AgentError) -> Self {
        Self::Agent(err)
    }
}

impl From<ApiError> for AgentError {
    /// A `Migrated` or `Conflict` that nobody handled is a permanent refusal.
    fn from(err: ApiError) -> Self {
        match err {
            ApiError::Agent(err) => err,
            ApiError::Migrated(_) => AgentError::Remote {
                status: Some(400),
                code: None,
                message: "the chat moved more than once".to_owned(),
            },
            ApiError::Conflict => AgentError::Remote {
                status: Some(409),
                code: None,
                message: "another program is reading this bot's updates".to_owned(),
            },
        }
    }
}

/// Whether a call reads or sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Call {
    /// Safe to repeat: retried inside the call.
    Read,
    /// Posts something: one attempt, because the engine owns delivery retries.
    Send,
}

/// A request body.
pub(crate) enum Body {
    /// A JSON object.
    Json(Value),
    /// A multipart form, with the time the whole upload may take.
    Form(Form, Duration),
}

/// A user or bot.
#[derive(Debug, Deserialize)]
pub(crate) struct Bot {
    pub id: i64,
    #[serde(default)]
    #[allow(dead_code)]
    pub username: Option<String>,
}

/// A chat, as `getChat` and updates describe it.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

/// The bot's standing in a chat.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatMember {
    pub status: String,
    #[serde(default)]
    pub can_post_messages: Option<bool>,
}

/// One `my_chat_member` update: the bot's standing in a chat changed.
#[derive(Debug, Clone)]
pub(crate) struct ChatMemberChange {
    pub chat: Chat,
    pub member: ChatMember,
}

/// The chat a sent message landed in.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SentChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// A message the Bot API accepted.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SentMessage {
    pub message_id: i64,
    pub chat: SentChat,
}

/// A text message to send.
pub(crate) struct SendText<'a> {
    pub chat_id: i64,
    pub text: &'a str,
    pub silent: bool,
    pub protect_content: bool,
    pub no_link_preview: bool,
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<ErrorParameters>,
}

#[derive(Deserialize)]
struct ErrorParameters {
    #[serde(default)]
    retry_after: Option<u64>,
    #[serde(default)]
    migrate_to_chat_id: Option<i64>,
}

fn error_body(excerpt: Option<&str>) -> Option<ErrorBody> {
    serde_json::from_str(excerpt?).ok()
}

/// Whether a `400` description says the chat is gone or the bot lacks the right to post.
fn chat_lost(description: &str) -> bool {
    let text = description.to_lowercase();
    ["chat not found", "not enough rights", "no rights", "admin"]
        .iter()
        .any(|needle| text.contains(needle))
}

/// The one error hook every call uses. The decision rests on the HTTP status and on
/// `parameters` wherever it can; the one match on `description` text is [`chat_lost`].
pub(crate) fn hook(status: u16, excerpt: Option<&str>) -> Option<AgentError> {
    let body = error_body(excerpt);
    match status {
        429 => body
            .and_then(|b| b.parameters)
            .and_then(|p| p.retry_after)
            .map(|secs| AgentError::RateLimited {
                retry_after: Some(Duration::from_secs(secs)),
            }),
        401 => Some(AgentError::Authentication {
            message: "the bot token was revoked or is wrong".to_owned(),
        }),
        403 => Some(AgentError::AccountUnreachable {
            message: "the bot was removed from the chat or blocked".to_owned(),
        }),
        400 => {
            let description = body.and_then(|b| b.description).unwrap_or_default();
            let refused = if description.is_empty() {
                "the Bot API refused the request".to_owned()
            } else {
                description.clone()
            };
            Some(if chat_lost(&description) {
                AgentError::AccountUnreachable {
                    message: "the bot cannot reach or post to the chat".to_owned(),
                }
            } else {
                AgentError::Remote {
                    status: Some(400),
                    code: None,
                    message: refused,
                }
            })
        }
        _ => None,
    }
}

/// The Bot API for one bot.
pub(crate) struct Api<'a> {
    http: &'a reqwest::Client,
    base: &'a Url,
    token: &'a SecretString,
    clock: Arc<dyn Clock>,
    upload_timeout: Duration,
}

impl<'a> Api<'a> {
    pub(crate) fn new(
        http: &'a reqwest::Client,
        base: &'a Url,
        token: &'a SecretString,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            http,
            base,
            token,
            clock,
            upload_timeout: Duration::from_mins(10),
        }
    }

    /// How long one upload may take; an album gets this much per item.
    pub(crate) fn upload_timeout(mut self, timeout: Duration) -> Self {
        self.upload_timeout = timeout;
        self
    }

    fn url(&self, method: &str) -> Result<Url, HttpError> {
        let mut url = self.base.as_str().trim_end_matches('/').to_owned();
        url.push_str("/bot");
        url.push_str(self.token.expose_secret());
        url.push('/');
        url.push_str(method);
        Url::parse(&url).map_err(|_| HttpError::Permanent("invalid Bot API address".to_owned()))
    }

    /// The only caller of `send_with_retry`, and the only place a URL is built.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        method: &str,
        call: Call,
        body: Body,
    ) -> Result<T, ApiError> {
        let (policy, idempotency) = match call {
            Call::Read => (
                RetryPolicy {
                    max_attempts: 3,
                    base_delay: Duration::from_millis(250),
                    max_delay: Duration::from_secs(2),
                    max_retry_after: Duration::ZERO,
                    budget: Duration::from_secs(10),
                },
                Idempotency::Idempotent,
            ),
            // The engine owns delivery retries; a `Retry-After` goes to it, never slept on here.
            Call::Send => (
                RetryPolicy {
                    max_attempts: 1,
                    max_retry_after: Duration::ZERO,
                    ..RetryPolicy::default()
                },
                Idempotency::NonIdempotent,
            ),
        };
        let mut json = None;
        let mut form = None;
        let mut timeout = None;
        match body {
            Body::Json(value) => json = Some(value),
            Body::Form(f, t) => {
                form = Some(f);
                timeout = Some(t);
            }
        }
        let make = || {
            let builder = self.http.post(self.url(method)?);
            let builder = match (&json, form.take()) {
                (Some(value), _) => builder.json(value),
                (None, Some(form)) => builder.multipart(form),
                (None, None) => {
                    return Err(HttpError::Permanent(
                        "the request body cannot be sent twice".to_owned(),
                    ));
                }
            };
            let builder = match timeout {
                Some(timeout) => builder.timeout(timeout),
                None => builder,
            };
            builder
                .build()
                .map_err(|_| HttpError::Permanent("the request could not be built".to_owned()))
        };
        let clock = Arc::clone(&self.clock);
        let result = send_with_retry(
            RetryRequest {
                client: self.http.into(),
                policy: &policy,
                idempotency,
                headers: &RateLimitHeaders::default(),
                now: Arc::new(move || clock.now()),
                limiter: None,
                sensitivity: Sensitivity::Opaque,
            },
            &mut Jitter::from_entropy(),
            make,
        )
        .await;
        let response = match result {
            Ok(response) => response,
            Err(err) => return Err(Self::fail(err)),
        };
        let unreadable = |message: &str| match call {
            Call::Send => AgentError::OutcomeUnknown {
                message: message.to_owned(),
            },
            Call::Read => AgentError::Remote {
                status: None,
                code: None,
                message: message.to_owned(),
            },
        };
        let envelope: Envelope<T> = response
            .json()
            .await
            .map_err(|_| unreadable("the Bot API answer could not be read"))?;
        match (envelope.ok, envelope.result) {
            (true, Some(result)) => Ok(result),
            _ => Err(unreadable("the Bot API answer had no result").into()),
        }
    }

    /// Maps a failed call. No phase turns a connect failure into `OutcomeUnknown`: nothing
    /// left the server, and `NonIdempotent` already reports everything after connect.
    fn fail(err: HttpError) -> ApiError {
        if let HttpError::Status {
            status,
            body_excerpt,
            ..
        } = &err
        {
            if *status == 409 {
                return ApiError::Conflict;
            }
            if let Some(id) = error_body(body_excerpt.as_deref())
                .and_then(|b| b.parameters)
                .and_then(|p| p.migrate_to_chat_id)
            {
                return ApiError::Migrated(id);
            }
        }
        let hook: &ErrorHook = &hook;
        ApiError::Agent(classify(err, Phase::BeforeSend, Some(hook)))
    }

    async fn read<T: DeserializeOwned>(&self, method: &str, body: Value) -> Result<T, ApiError> {
        self.post(method, Call::Read, Body::Json(body)).await
    }

    /// Sends plain text (no `parse_mode`: what the user wrote is what goes out).
    pub(crate) async fn send_message(&self, send: &SendText<'_>) -> Result<SentMessage, ApiError> {
        let mut body = json!({"chat_id": send.chat_id, "text": send.text});
        if send.silent {
            body["disable_notification"] = json!(true);
        }
        if send.protect_content {
            body["protect_content"] = json!(true);
        }
        if send.no_link_preview {
            body["link_preview_options"] = json!({"is_disabled": true});
        }
        self.post("sendMessage", Call::Send, Body::Json(body)).await
    }

    /// Sends one file with `method` (`sendPhoto`, `sendVideo`, ...).
    pub(crate) async fn send_media(
        &self,
        method: &str,
        form: Form,
    ) -> Result<SentMessage, ApiError> {
        self.post(method, Call::Send, Body::Form(form, self.upload_timeout))
            .await
    }

    /// Sends an album of `items` files in one request, which may carry ten large files, so it
    /// gets the upload time per item.
    pub(crate) async fn send_album(
        &self,
        form: Form,
        items: usize,
    ) -> Result<Vec<SentMessage>, ApiError> {
        let items = u32::try_from(items).unwrap_or(u32::MAX).max(1);
        self.post(
            "sendMediaGroup",
            Call::Send,
            Body::Form(form, self.upload_timeout.saturating_mul(items)),
        )
        .await
    }

    /// The bot itself.
    pub(crate) async fn get_me(&self) -> Result<Bot, ApiError> {
        self.read("getMe", json!({})).await
    }

    /// The bot's recent `my_chat_member` updates, read without confirming any: no `offset`, so
    /// nothing is consumed, and no `allowed_updates`, which would stay in force for every
    /// other program reading the bot. Oldest first.
    pub(crate) async fn peek_updates(&self) -> Result<Vec<ChatMemberChange>, ApiError> {
        let updates: Vec<Value> = self
            .read("getUpdates", json!({"timeout": 0, "limit": 100}))
            .await?;
        Ok(updates
            .into_iter()
            .filter_map(|update| {
                let change = update.get("my_chat_member")?;
                Some(ChatMemberChange {
                    chat: serde_json::from_value(change.get("chat")?.clone()).ok()?,
                    member: serde_json::from_value(change.get("new_chat_member")?.clone()).ok()?,
                })
            })
            .collect())
    }

    async fn chat_once(&self, chat: &ChatRef) -> Result<Chat, ApiError> {
        let id = match chat {
            ChatRef::Username(name) => json!(format!("@{name}")),
            ChatRef::Id(id) => json!(id),
        };
        self.read("getChat", json!({"chat_id": id})).await
    }

    /// A chat, with its current ID: a group upgraded to a supergroup is followed once.
    pub(crate) async fn get_chat(&self, chat: &ChatRef) -> Result<Chat, ApiError> {
        match self.chat_once(chat).await {
            Err(ApiError::Migrated(id)) => self.chat_once(&ChatRef::Id(id)).await,
            other => other,
        }
    }

    /// The bot's (or another user's) standing in a chat.
    pub(crate) async fn get_chat_member(
        &self,
        chat: i64,
        user: i64,
    ) -> Result<ChatMember, ApiError> {
        self.read("getChatMember", json!({"chat_id": chat, "user_id": user}))
            .await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::RetryClass;

    use super::*;

    fn body(code: u16, description: &str) -> String {
        json!({"ok": false, "error_code": code, "description": description}).to_string()
    }

    fn mapped(status: u16, excerpt: &str) -> AgentError {
        hook(status, Some(excerpt)).unwrap()
    }

    #[test]
    fn rate_limit_reads_the_body() {
        let excerpt = json!({"ok": false, "parameters": {"retry_after": 7}}).to_string();
        assert_eq!(
            mapped(429, &excerpt),
            AgentError::RateLimited {
                retry_after: Some(Duration::from_secs(7))
            }
        );
        // No JSON: the default mapping reads the header.
        assert!(hook(429, Some("busy")).is_none());
        assert!(hook(429, None).is_none());
    }

    #[test]
    fn status_rows() {
        let revoked = mapped(401, &body(401, "Unauthorized"));
        assert_eq!(revoked.retry_class(), RetryClass::NeedsReconnect);
        assert!(matches!(revoked, AgentError::Authentication { .. }));

        let removed = mapped(403, &body(403, "Forbidden: bot was kicked"));
        assert!(matches!(removed, AgentError::AccountUnreachable { .. }));
        assert_eq!(removed.retry_class(), RetryClass::NeedsReconnect);

        for lost in [
            "Bad Request: chat not found",
            "Bad Request: Chat Not Found",
            "Bad Request: not enough rights to send text messages to the chat",
            "Bad Request: have no rights to send a message",
            "Bad Request: CHAT_ADMIN_REQUIRED",
        ] {
            assert!(
                matches!(
                    mapped(400, &body(400, lost)),
                    AgentError::AccountUnreachable { .. }
                ),
                "{lost}"
            );
        }

        let other = mapped(400, &body(400, "Bad Request: message is too long"));
        assert_eq!(
            other,
            AgentError::Remote {
                status: Some(400),
                code: None,
                message: "Bad Request: message is too long".to_owned()
            }
        );
        assert_eq!(other.retry_class(), RetryClass::Permanent);

        // Anything else, and anything unreadable, is the kit's default.
        assert!(hook(500, Some("oops")).is_none());
        assert!(matches!(
            hook(400, Some("<html>")).unwrap(),
            AgentError::Remote {
                status: Some(400),
                ..
            }
        ));
    }

    #[test]
    fn conflict_and_migration_are_not_agent_errors() {
        let status = |code: u16, body: &str| HttpError::Status {
            status: code,
            retry_after: None,
            body_excerpt: Some(body.to_owned()),
        };
        assert!(matches!(
            Api::fail(status(
                409,
                &body(409, "Conflict: terminated by other getUpdates")
            )),
            ApiError::Conflict
        ));
        let moved = json!({
            "ok": false,
            "description": "Bad Request: group chat was upgraded to a supergroup chat",
            "parameters": {"migrate_to_chat_id": -1_001_234_567_890_i64}
        })
        .to_string();
        assert!(matches!(
            Api::fail(status(400, &moved)),
            ApiError::Migrated(-1_001_234_567_890)
        ));
    }
}
