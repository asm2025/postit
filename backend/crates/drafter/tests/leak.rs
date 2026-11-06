//! `Debug` never shows a secret or the user's prompt: not the sealed parts of a provider, not
//! its stored row, not a chat request or answer, not a provider, and not any failure.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use chrono::Utc;
use postit_config::HttpSettings;
use postit_core::AiProviderId;
use postit_data::ai_providers::{AiProvider, AiProviderRepo};
use postit_drafter::{
    Anthropic, ChatMessage, ChatRequest, ChatResponse, CreateProvider, DrafterError, FailureReason,
    Finish, LlmError, OpenAiCompatible, OutputMode, Part, ProviderConfig, ProviderOptions,
    ProviderSecret, Role, UpdateProvider, Usage,
};
use postit_http::testkit::leak::assert_no_leak;
use postit_http::{AddressPolicy, build_public_client};
use postit_vault::Vault;
use secrecy::SecretString;
use serde_json::json;
use tokio::time::Instant;
use url::Url;
use uuid::Uuid;

const KEY: &str = "sk-test-LEAK-ME-0123456789";
const HEADER: &str = "hdr-LEAK-ME";
const PROMPT: &str = "PROMPT-LEAK-ME about the spring menu";
const SYSTEM: &str = "SYSTEM-LEAK-ME brand voice";
const ANSWER: &str = "ANSWER-LEAK-ME spring is here";
const IMAGE: &[u8] = b"\xff\xd8\xff\xe0IMAGE-LEAK-ME";

const SECRETS: [&str; 6] = [KEY, HEADER, PROMPT, SYSTEM, ANSWER, "IMAGE-LEAK-ME"];

fn secret() -> ProviderSecret {
    let mut headers = BTreeMap::new();
    headers.insert("X-Secret".to_owned(), SecretString::from(HEADER.to_owned()));
    ProviderSecret {
        api_key: Some(SecretString::from(KEY.to_owned())),
        headers,
    }
}

fn config() -> ProviderConfig {
    ProviderConfig {
        options: ProviderOptions {
            base_url: Some(Url::parse("https://llm.example.com/v1").unwrap()),
            headers: vec!["X-Secret".to_owned()],
            ..ProviderOptions::default()
        },
        default_model: Some("m-1".to_owned()),
        ..ProviderConfig::default()
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "m-1".to_owned(),
        system: SYSTEM.to_owned(),
        messages: vec![
            ChatMessage {
                role: Role::User,
                parts: vec![
                    Part::Text(PROMPT.to_owned()),
                    Part::Image {
                        jpeg: Bytes::from_static(IMAGE),
                    },
                ],
            },
            ChatMessage {
                role: Role::Assistant,
                parts: vec![Part::Text(ANSWER.to_owned())],
            },
        ],
        temperature: Some(0.5),
        max_tokens: 8_192,
        output: OutputMode::JsonSchema {
            name: "draft".to_owned(),
            schema: json!({ "description": PROMPT }),
        },
        deadline: Instant::now() + Duration::from_secs(60),
    }
}

fn client() -> postit_http::PublicClient {
    build_public_client(
        &HttpSettings {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            user_agent: "postit-drafter-leak-test/0".to_owned(),
            extra_ca_files: Vec::<PathBuf>::new(),
        },
        AddressPolicy {
            allow_private: false,
        },
    )
    .unwrap()
}

const REASONS: [FailureReason; 9] = [
    FailureReason::KeyRejected,
    FailureReason::ModelNotFound,
    FailureReason::RateLimited,
    FailureReason::BadRequest,
    FailureReason::Unavailable,
    FailureReason::Timeout,
    FailureReason::AddressRefused,
    FailureReason::InvalidResponse,
    FailureReason::InvalidOutput,
];

#[test]
fn debug_shows_no_secret_and_no_prompt() {
    let mut outputs: Vec<(String, String)> = Vec::new();
    let mut push = |label: &str, text: String| outputs.push((label.to_owned(), text));

    push("ProviderSecret", format!("{:?}", secret()));
    push("ProviderConfig", format!("{:?}", config()));

    let vault =
        Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-leak-key".to_vec())]).unwrap();
    let id = AiProviderId::from(Uuid::now_v7());
    let sealed = vault
        .seal(&secret().to_value(), &AiProviderRepo::seal_context(id))
        .unwrap();
    push("Sealed", format!("{sealed:?}"));
    let row = AiProvider {
        id,
        provider_key: "custom".to_owned(),
        config: serde_json::to_value(config()).unwrap(),
        secret: Some(sealed),
        is_default: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    push("AiProvider", format!("{row:?}"));

    let req = request();
    push("ChatRequest", format!("{req:?}"));
    push("ChatRequest pretty", format!("{req:#?}"));
    for message in &req.messages {
        push("ChatMessage", format!("{message:?}"));
        for part in &message.parts {
            push("Part", format!("{part:?}"));
        }
    }
    push(
        "ChatResponse",
        format!(
            "{:?}",
            ChatResponse {
                text: ANSWER.to_owned(),
                usage: Some(Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                }),
                finish: Finish::Length,
            }
        ),
    );

    for reason in REASONS {
        let plain = LlmError::new(reason);
        let delayed = LlmError {
            reason,
            retry_after: Some(Duration::from_secs(7)),
        };
        for error in [plain, delayed] {
            push("LlmError", format!("{error:?}"));
            push("LlmError display", error.to_string());
            let wrapped = DrafterError::ProviderFailed(error);
            push("DrafterError", format!("{wrapped:?}"));
            push("DrafterError display", wrapped.to_string());
        }
    }

    push(
        "CreateProvider::Preset",
        format!(
            "{:?}",
            CreateProvider::Preset {
                key: "openai".to_owned(),
                api_key: Some(SecretString::from(KEY.to_owned())),
                default_model: None,
            }
        ),
    );
    push(
        "CreateProvider::Pasted",
        format!(
            "{:?}",
            CreateProvider::Pasted {
                entry: json!({ "options": { "apiKey": KEY, "headers": { "X-Secret": HEADER } } }),
            }
        ),
    );
    let mut headers = BTreeMap::new();
    headers.insert("X-Secret".to_owned(), SecretString::from(HEADER.to_owned()));
    push(
        "UpdateProvider",
        format!(
            "{:?}",
            UpdateProvider {
                api_key: Some(SecretString::from(KEY.to_owned())),
                headers: Some(headers),
                ..UpdateProvider::default()
            }
        ),
    );

    push(
        "OpenAiCompatible",
        format!(
            "{:?}",
            OpenAiCompatible::new(
                client(),
                Url::parse("https://llm.example.com/v1").unwrap(),
                secret(),
                &config(),
                None,
            )
        ),
    );
    push(
        "Anthropic",
        format!(
            "{:?}",
            Anthropic::new(
                client(),
                Url::parse("https://api.anthropic.com/v1").unwrap(),
                secret(),
                &config(),
            )
        ),
    );

    let borrowed: Vec<(&str, &str)> = outputs
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    assert_no_leak(&SECRETS, &borrowed);
    // The sizes are there, so the redaction is not an empty print.
    let request_debug = &borrowed
        .iter()
        .find(|(label, _)| *label == "ChatRequest")
        .unwrap()
        .1;
    assert!(request_debug.contains("system_chars"), "{request_debug}");
}
