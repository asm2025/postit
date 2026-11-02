//! A wiremock Bot API and the contexts to run the agent against it.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use postit_agent_kit::testkit::TestContext;
use postit_agent_telegram::{TelegramAgent, TelegramConfig};
use postit_core::{
    AccountId, AccountRef, Agent, AgentClient, CredentialFields, Credentials, PlatformId,
};
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// The token every test bot has.
pub const TOKEN: &str = "123456:ABC-def_1";
/// The bot's own user ID.
pub const BOT_ID: i64 = 1000;

pub fn fields(chats: Option<&str>) -> CredentialFields {
    let mut plain = Map::new();
    if let Some(chats) = chats {
        plain.insert("chats".to_owned(), json!(chats));
    }
    let secret = BTreeMap::from([("bot_token".to_owned(), SecretString::from(TOKEN))]);
    CredentialFields::from_parts(plain, secret)
}

/// A fake Bot API and a test context holding the bot's credentials.
pub struct Bot {
    pub server: MockServer,
    pub ctx: TestContext,
}

impl Bot {
    pub async fn start() -> Self {
        Self::with_chats(None).await
    }

    pub async fn with_chats(chats: Option<&str>) -> Self {
        Self {
            server: MockServer::start().await,
            ctx: TestContext::new().with_credentials(Credentials::Fields(fields(chats))),
        }
    }

    pub fn config(&self) -> TelegramConfig {
        TelegramConfig {
            api_base: Url::parse(&self.server.uri()).unwrap(),
            ..TelegramConfig::default()
        }
    }

    pub fn agent(&self) -> TelegramAgent {
        TelegramAgent::new(self.config())
    }

    pub fn client(&self) -> Box<dyn AgentClient> {
        self.agent()
            .create_client(self.ctx.agent_context(Value::Null))
            .unwrap()
    }

    /// `getMe` answers with the test bot.
    pub async fn bot(&self) {
        self.ok(
            "getMe",
            json!({"id": BOT_ID, "is_bot": true, "username": "postit_bot"}),
        )
        .await;
    }

    pub async fn respond(&self, method_name: &str, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(format!("/bot{TOKEN}/{method_name}")))
            .respond_with(template)
            .mount(&self.server)
            .await;
    }

    pub async fn ok(&self, method_name: &str, result: Value) {
        self.respond(
            method_name,
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": result})),
        )
        .await;
    }

    pub async fn fail(&self, method_name: &str, status: u16, description: &str) {
        self.respond(
            method_name,
            ResponseTemplate::new(status).set_body_json(json!({
                "ok": false,
                "error_code": status,
                "description": description,
            })),
        )
        .await;
    }

    /// Every request made to `method_name`, in order.
    pub async fn requests(&self, method_name: &str) -> Vec<Request> {
        let want = format!("/bot{TOKEN}/{method_name}");
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path() == want)
            .collect()
    }

    /// The JSON body of each request made to `method_name`.
    pub async fn bodies(&self, method_name: &str) -> Vec<Value> {
        self.requests(method_name)
            .await
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
            .collect()
    }
}

pub fn account(remote_id: &str) -> AccountRef {
    AccountRef {
        platform: PlatformId::new("telegram").unwrap(),
        account: AccountId::from_uuid(Uuid::now_v7()),
        remote_id: remote_id.to_owned(),
    }
}

pub fn chat(id: i64, kind: &str, title: &str, username: Option<&str>) -> Value {
    json!({"id": id, "type": kind, "title": title, "username": username})
}

/// A `my_chat_member` update: the bot's standing in `chat` became `status`.
pub fn member_update(chat: &Value, status: &str, can_post: Option<bool>) -> Value {
    json!({
        "update_id": 1,
        "my_chat_member": {
            "chat": chat,
            "from": {"id": 5, "is_bot": false},
            "date": 1,
            "old_chat_member": {"status": "left", "user": {"id": BOT_ID, "is_bot": true}},
            "new_chat_member": {
                "status": status,
                "can_post_messages": can_post,
                "user": {"id": BOT_ID, "is_bot": true}
            }
        }
    })
}
