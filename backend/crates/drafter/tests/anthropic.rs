//! The native Anthropic protocol against wiremock: request shape, base URL handling, reading,
//! error mapping, and model paging.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use postit_config::HttpSettings;
use postit_drafter::anthropic::{ANTHROPIC_VERSION, Anthropic};
use postit_drafter::{
    ChatMessage, ChatRequest, FailureReason, Finish, LlmProvider, OutputMode, Part, Protocol,
    ProviderSecret, Role, Usage, normalize_base,
};
use postit_http::{AddressPolicy, PublicClient, build_public_client};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::time::Instant;
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROMPT: &str = "PROMPT-SECRET-TEXT about our bakery";
const IMAGE: &[u8] = b"\xff\xd8\xff\xe0JPEG-SECRET-BYTES";

fn client() -> PublicClient {
    build_public_client(
        &HttpSettings {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            user_agent: "postit-drafter-test/0".to_owned(),
            extra_ca_files: Vec::<PathBuf>::new(),
        },
        AddressPolicy {
            allow_private: true,
        },
    )
    .unwrap()
}

fn secret() -> ProviderSecret {
    ProviderSecret {
        api_key: Some(SecretString::from("sk-ant-test".to_owned())),
        ..ProviderSecret::default()
    }
}

fn provider_at(base: &str) -> Anthropic {
    let base = normalize_base(Protocol::Anthropic, Url::parse(base).unwrap());
    Anthropic::new(client(), base, secret())
}

fn provider(server: &MockServer) -> Anthropic {
    provider_at(&server.uri())
}

fn request(output: OutputMode, left: Duration) -> ChatRequest {
    ChatRequest {
        model: "claude-x".to_owned(),
        system: "You write posts.".to_owned(),
        messages: vec![ChatMessage {
            role: Role::User,
            parts: vec![
                Part::Text(PROMPT.to_owned()),
                Part::Image {
                    jpeg: Bytes::from_static(IMAGE),
                },
            ],
        }],
        temperature: Some(0.5),
        max_tokens: 2_048,
        output,
        deadline: Instant::now() + left,
    }
}

fn text_request() -> ChatRequest {
    request(OutputMode::Text, Duration::from_secs(10))
}

fn message(content: &Value, stop: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant",
        "content": content,
        "stop_reason": stop,
        "usage": {"input_tokens": 11, "output_tokens": 6}
    }))
}

async fn mount(server: &MockServer, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(template)
        .mount(server)
        .await;
}

async fn failure(status: u16) -> postit_drafter::LlmError {
    let server = MockServer::start().await;
    mount(
        &server,
        ResponseTemplate::new(status).set_body_string("BODY-SECRET echo"),
    )
    .await;
    provider(&server).chat(&text_request()).await.unwrap_err()
}

#[tokio::test]
async fn request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-ant-test"))
        .and(header("anthropic-version", ANTHROPIC_VERSION))
        .and(header("content-type", "application/json"))
        .respond_with(message(
            &json!([{"type": "text", "text": "hi"}]),
            "end_turn",
        ))
        .mount(&server)
        .await;
    let schema = json!({"type": "object", "properties": {"variants": {"type": "array"}}});
    let mode = OutputMode::JsonSchema {
        name: "variants".to_owned(),
        schema: schema.clone(),
    };
    provider(&server)
        .chat(&request(mode, Duration::from_secs(10)))
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["model"], "claude-x");
    assert_eq!(body["max_tokens"], 2_048);
    assert_eq!(body["system"], "You write posts.");
    assert!(
        body.get("temperature").is_none(),
        "removed in the pinned API version: {body}"
    );
    assert_eq!(
        body["output_config"],
        json!({"format": {"type": "json_schema", "schema": schema}})
    );
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "system is not a message");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(
        messages[0]["content"][0],
        json!({"type": "text", "text": PROMPT})
    );
    assert_eq!(
        messages[0]["content"][1],
        json!({"type": "image", "source": {
            "type": "base64", "media_type": "image/jpeg", "data": STANDARD.encode(IMAGE)
        }})
    );
}

#[tokio::test]
async fn text_and_json_object_modes_send_no_output_config() {
    for mode in [OutputMode::Text, OutputMode::JsonObject] {
        let server = MockServer::start().await;
        mount(
            &server,
            message(&json!([{"type": "text", "text": "{}"}]), "end_turn"),
        )
        .await;
        provider(&server)
            .chat(&request(mode, Duration::from_secs(10)))
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(body.get("output_config").is_none(), "{body}");
    }
}

#[tokio::test]
async fn base_with_v1_reaches_v1_messages() {
    for suffix in ["", "/", "/v1", "/v1/"] {
        let server = MockServer::start().await;
        mount(
            &server,
            message(&json!([{"type": "text", "text": "x"}]), "end_turn"),
        )
        .await;
        provider_at(&format!("{}{suffix}", server.uri()))
            .chat(&text_request())
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "{suffix:?}");
        assert_eq!(requests[0].url.path(), "/v1/messages", "{suffix:?}");
    }
}

#[tokio::test]
async fn text_blocks_joined_and_usage_read() {
    let server = MockServer::start().await;
    let content = json!([
        {"type": "thinking", "thinking": "THINK-SECRET"},
        {"type": "text", "text": "{\"vari"},
        {"type": "text", "text": "ants\":[]}"}
    ]);
    mount(&server, message(&content, "end_turn")).await;
    let response = provider(&server)
        .chat(&request(OutputMode::JsonObject, Duration::from_secs(10)))
        .await
        .unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
    assert_eq!(response.finish, Finish::Stop);
    assert_eq!(
        response.usage,
        Some(Usage {
            input_tokens: 11,
            output_tokens: 6
        })
    );
}

#[tokio::test]
async fn stop_reason_max_tokens_is_length() {
    let server = MockServer::start().await;
    mount(
        &server,
        message(&json!([{"type": "text", "text": "{\"a"}]), "max_tokens"),
    )
    .await;
    let response = provider(&server).chat(&text_request()).await.unwrap();
    assert_eq!(response.finish, Finish::Length);
    assert_eq!(response.text, "{\"a");
}

#[tokio::test]
async fn refusal_stop_reason_is_refused() {
    let server = MockServer::start().await;
    mount(&server, message(&json!([]), "refusal")).await;
    let response = provider(&server).chat(&text_request()).await.unwrap();
    assert_eq!(response.finish, Finish::Refused);
    assert_eq!(response.text, "");
}

#[tokio::test]
async fn malformed_answer_is_invalid_response() {
    for body in [json!({"stop_reason": "end_turn"}), json!([1])] {
        let server = MockServer::start().await;
        mount(&server, ResponseTemplate::new(200).set_body_json(body)).await;
        let err = provider(&server).chat(&text_request()).await.unwrap_err();
        assert_eq!(err.reason, FailureReason::InvalidResponse);
    }
}

#[tokio::test]
async fn error_body_maps_like_openai() {
    for (status, reason) in [
        (401, FailureReason::KeyRejected),
        (404, FailureReason::ModelNotFound),
        (400, FailureReason::BadRequest),
        (529, FailureReason::Unavailable),
    ] {
        let err = failure(status).await;
        assert_eq!(err.reason, reason, "{status}");
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains("BODY-SECRET"), "{shown}");
    }

    let server = MockServer::start().await;
    mount(
        &server,
        ResponseTemplate::new(429).insert_header("retry-after", "600"),
    )
    .await;
    let err = provider(&server)
        .chat(&request(OutputMode::Text, Duration::from_secs(2)))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::RateLimited);
    assert_eq!(err.retry_after, Some(Duration::from_secs(600)));
}

fn page(ids: &[&str], has_more: bool) -> ResponseTemplate {
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "display_name": format!("Name {id}"), "type": "model"}))
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({
        "data": data,
        "has_more": has_more,
        "first_id": ids.first(),
        "last_id": ids.last(),
    }))
}

#[tokio::test]
async fn models_follows_has_more_up_to_ten_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-ant-test"))
        .and(header("anthropic-version", ANTHROPIC_VERSION))
        .respond_with(|req: &wiremock::Request| {
            let after = req
                .url
                .query_pairs()
                .find(|(k, _)| k == "after_id")
                .map(|(_, v)| v.into_owned());
            let n: usize = after
                .as_deref()
                .map_or(0, |a| a.trim_start_matches('m').parse().unwrap());
            let id = format!("m{}", n + 1);
            page(&[&id], true)
        })
        .mount(&server)
        .await;
    let models = provider(&server)
        .models(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(models.len(), 10, "eleven pages served, ten read");
    assert_eq!(models[0].id, "m1");
    assert_eq!(models[0].name.as_deref(), Some("Name m1"));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 10);
    for (i, r) in requests.iter().enumerate() {
        let pairs: BTreeMap<_, _> = r.url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("limit").map(String::as_str), Some("1000"));
        if i == 0 {
            assert!(
                !pairs.contains_key("after_id"),
                "first page has no after_id"
            );
        } else {
            assert_eq!(pairs.get("after_id"), Some(&format!("m{i}")));
        }
    }
}

#[tokio::test]
async fn models_stop_when_has_more_is_false() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(query_param("limit", "1000"))
        .respond_with(page(&["a", "b"], false))
        .mount(&server)
        .await;
    let models = provider(&server)
        .models(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn models_share_one_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(page(&["a"], true).set_delay(Duration::from_millis(250)))
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = provider(&server)
        .models(Instant::now() + Duration::from_millis(600))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn debug_hides_secrets() {
    let shown = format!("{:?}", provider_at("https://api.anthropic.com"));
    assert!(!shown.contains("sk-ant-test"), "{shown}");
}
