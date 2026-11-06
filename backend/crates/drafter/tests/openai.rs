//! The OpenAI-compatible protocol against wiremock: request shape, tolerant reading, error
//! mapping, retries, and the deadline.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use postit_config::HttpSettings;
use postit_drafter::openai::OpenAiCompatible;
use postit_drafter::{
    ChatMessage, ChatRequest, ChatResponse, FailureReason, Finish, LlmError, LlmProvider,
    OutputMode, Part, ProviderConfig, ProviderOptions, ProviderSecret, Role, TokenParam, Usage,
    preset,
};
use postit_http::{AddressPolicy, PublicClient, build_public_client};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::time::Instant;
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PROMPT: &str = "PROMPT-SECRET-TEXT about our bakery";
const IMAGE: &[u8] = b"\xff\xd8\xff\xe0JPEG-SECRET-BYTES";

fn settings() -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(30),
        user_agent: "postit-drafter-test/0".to_owned(),
        extra_ca_files: Vec::<PathBuf>::new(),
    }
}

fn client() -> PublicClient {
    build_public_client(
        &settings(),
        AddressPolicy {
            allow_private: true,
        },
    )
    .unwrap()
}

fn secret() -> ProviderSecret {
    let mut headers = BTreeMap::new();
    headers.insert("X-Org".to_owned(), SecretString::from("org-1".to_owned()));
    ProviderSecret {
        api_key: Some(SecretString::from("sk-test-key".to_owned())),
        headers,
    }
}

fn config() -> ProviderConfig {
    let mut query = BTreeMap::new();
    query.insert("api-version".to_owned(), "2026-01-01".to_owned());
    ProviderConfig {
        options: ProviderOptions {
            headers: vec!["X-Org".to_owned()],
            query,
            ..ProviderOptions::default()
        },
        ..ProviderConfig::default()
    }
}

fn base(server: &MockServer) -> Url {
    Url::parse(&format!("{}/v1", server.uri())).unwrap()
}

fn provider(server: &MockServer) -> OpenAiCompatible {
    OpenAiCompatible::new(client(), base(server), secret(), &config(), None)
}

fn request(output: OutputMode, left: Duration) -> ChatRequest {
    ChatRequest {
        model: "m-1".to_owned(),
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
        temperature: None,
        max_tokens: 4_096,
        output,
        deadline: Instant::now() + left,
    }
}

fn schema_mode() -> OutputMode {
    OutputMode::JsonSchema {
        name: "variants".to_owned(),
        schema: json!({"type": "object", "properties": {"variants": {"type": "array"}}}),
    }
}

fn text_request() -> ChatRequest {
    request(OutputMode::Text, Duration::from_secs(10))
}

fn completion(content: &Value) -> Value {
    json!({
        "id": "c-1",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19}
    })
}

fn ok_body() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(completion(&json!("{\"variants\":[]}")))
}

async fn mount_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_body())
        .mount(server)
        .await;
}

async fn body_of(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "one request");
    serde_json::from_slice(&requests[0].body).unwrap()
}

async fn chat_with(
    server: &MockServer,
    template: ResponseTemplate,
) -> Result<ChatResponse, LlmError> {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(template)
        .mount(server)
        .await;
    provider(server).chat(&text_request()).await
}

fn reason(result: &Result<ChatResponse, LlmError>) -> FailureReason {
    match result {
        Err(e) => e.reason,
        Ok(r) => panic!("expected an error, got {r:?}"),
    }
}

#[tokio::test]
async fn request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test-key"))
        .and(header("x-org", "org-1"))
        .and(header("content-type", "application/json"))
        .and(query_param("api-version", "2026-01-01"))
        .respond_with(ok_body())
        .mount(&server)
        .await;

    let response = provider(&server)
        .chat(&request(schema_mode(), Duration::from_secs(10)))
        .await
        .unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
    assert_eq!(response.finish, Finish::Stop);
    assert_eq!(
        response.usage,
        Some(Usage {
            input_tokens: 12,
            output_tokens: 7
        })
    );

    let body = body_of(&server).await;
    assert_eq!(body["model"], "m-1");
    assert_eq!(body["max_tokens"], 4_096);
    assert!(body.get("max_completion_tokens").is_none(), "{body}");
    assert!(body.get("temperature").is_none(), "{body}");
    assert_eq!(
        body["response_format"],
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": "variants",
                "schema": {"type": "object", "properties": {"variants": {"type": "array"}}}
            }
        })
    );
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({"role": "system", "content": "You write posts."})
    );
    assert_eq!(messages[1]["role"], "user");
    let parts = messages[1]["content"].as_array().unwrap();
    assert_eq!(parts[0], json!({"type": "text", "text": PROMPT}));
    assert_eq!(
        parts[1],
        json!({
            "type": "image_url",
            "image_url": {"url": format!("data:image/jpeg;base64,{}", STANDARD.encode(IMAGE))}
        })
    );
}

#[tokio::test]
async fn openai_preset_sends_max_completion_tokens() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let provider = OpenAiCompatible::new(
        client(),
        base(&server),
        secret(),
        &config(),
        preset("openai"),
    );
    provider.chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert_eq!(body["max_completion_tokens"], 4_096);
    assert!(body.get("max_tokens").is_none(), "{body}");
}

#[tokio::test]
async fn config_token_param_overrides_preset() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let mut config = config();
    config.options.token_param = Some(TokenParam::MaxCompletionTokens);
    let provider = OpenAiCompatible::new(client(), base(&server), secret(), &config, None);
    provider.chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert_eq!(body["max_completion_tokens"], 4_096);
    assert!(body.get("max_tokens").is_none(), "{body}");
}

#[tokio::test]
async fn configured_temperature_sent() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let mut config = config();
    config.options.temperature = Some(0.25);
    let provider = OpenAiCompatible::new(client(), base(&server), secret(), &config, None);
    provider.chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert!(
        (body["temperature"].as_f64().unwrap() - 0.25).abs() < 1e-6,
        "{body}"
    );
}

/// A request's own temperature wins over the configured one; the configured one fills in
/// when the request has none (the drafter leaves it `None`), and the value is sent as typed.
#[tokio::test]
async fn request_temperature_wins_over_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_body())
        .mount(&server)
        .await;
    let mut config = config();
    config.options.temperature = Some(0.25);
    let provider = OpenAiCompatible::new(client(), base(&server), secret(), &config, None);
    let mut req = text_request();
    req.temperature = Some(0.7);
    provider.chat(&req).await.unwrap();
    provider.chat(&text_request()).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let sent: Vec<String> = requests
        .iter()
        .map(|r| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            body["temperature"].to_string()
        })
        .collect();
    assert_eq!(sent, ["0.7", "0.25"]);
}

#[tokio::test]
async fn json_object_mode_sends_type_json_object() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    provider(&server)
        .chat(&request(OutputMode::JsonObject, Duration::from_secs(10)))
        .await
        .unwrap();
    let body = body_of(&server).await;
    assert_eq!(body["response_format"], json!({"type": "json_object"}));
}

#[tokio::test]
async fn text_mode_sends_no_response_format() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    provider(&server).chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert!(body.get("response_format").is_none(), "{body}");
}

#[tokio::test]
async fn text_only_message_sends_string_content() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let mut req = text_request();
    req.messages = vec![
        ChatMessage {
            role: Role::User,
            parts: vec![Part::Text("first".to_owned())],
        },
        ChatMessage {
            role: Role::Assistant,
            parts: vec![Part::Text("answer".to_owned())],
        },
    ];
    provider(&server).chat(&req).await.unwrap();
    let body = body_of(&server).await;
    assert_eq!(
        body["messages"][1],
        json!({"role": "user", "content": "first"})
    );
    assert_eq!(
        body["messages"][2],
        json!({"role": "assistant", "content": "answer"})
    );
}

#[tokio::test]
async fn base_url_with_and_without_trailing_slash() {
    for suffix in ["/v1", "/v1/"] {
        let server = MockServer::start().await;
        mount_ok(&server).await;
        let base = Url::parse(&format!("{}{suffix}", server.uri())).unwrap();
        let provider = OpenAiCompatible::new(client(), base, secret(), &config(), None);
        provider.chat(&text_request()).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "{suffix}");
        assert_eq!(requests[0].url.path(), "/v1/chat/completions", "{suffix}");
    }
}

#[tokio::test]
async fn missing_usage_is_none() {
    let server = MockServer::start().await;
    let body = json!({
        "choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]
    });
    let response = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
        .await
        .unwrap();
    assert_eq!(response.text, "hi");
    assert_eq!(response.usage, None);
}

#[tokio::test]
async fn content_as_array_of_parts_is_joined() {
    let server = MockServer::start().await;
    let content = json!([
        {"type": "text", "text": "{\"vari"},
        {"type": "text", "text": "ants\":[]}"}
    ]);
    let response = chat_with(
        &server,
        ResponseTemplate::new(200).set_body_json(completion(&content)),
    )
    .await
    .unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
}

#[tokio::test]
async fn think_block_stripped_and_reasoning_content_ignored() {
    let server = MockServer::start().await;
    let body = json!({
        "choices": [{
            "message": {
                "content": "<think>\nThe user wants a post.\n</think>\n\n{\"variants\":[]}",
                "reasoning_content": "REASONING-TEXT"
            },
            "finish_reason": "stop"
        }]
    });
    let response = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
        .await
        .unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
}

#[tokio::test]
async fn fenced_json_extracted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion(&json!(
            "Here you go:\n```json\n{\"variants\":[]}\n```\nEnjoy!"
        ))))
        .mount(&server)
        .await;
    let response = provider(&server)
        .chat(&request(OutputMode::JsonObject, Duration::from_secs(10)))
        .await
        .unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
}

#[tokio::test]
async fn length_finish_reported() {
    let server = MockServer::start().await;
    let body = json!({
        "choices": [{"message": {"content": "{\"variants\":[{\"tar"}, "finish_reason": "length"}]
    });
    let response = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
        .await
        .unwrap();
    assert_eq!(response.finish, Finish::Length);
    assert_eq!(response.text, "{\"variants\":[{\"tar");
}

#[tokio::test]
async fn content_filter_and_refusal_are_refused() {
    let server = MockServer::start().await;
    let body = json!({
        "choices": [{"message": {"content": null, "refusal": "I can't help."}, "finish_reason": "stop"}]
    });
    let response = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
        .await
        .unwrap();
    assert_eq!(response.finish, Finish::Refused);
    assert_eq!(response.text, "");

    let server = MockServer::start().await;
    let body = json!({
        "choices": [{"message": {"content": ""}, "finish_reason": "content_filter"}]
    });
    let response = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
        .await
        .unwrap();
    assert_eq!(response.finish, Finish::Refused);
}

/// A protocol answer with nothing usable in it (empty `choices`, a choice without `message`)
/// is an empty answer, with its usage, for the drafter to repair; it is not a protocol error.
#[tokio::test]
async fn empty_choices_is_an_empty_answer() {
    for body in [
        json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 0}}),
        json!({"choices": [{"finish_reason": "stop"}], "usage": {"prompt_tokens": 12, "completion_tokens": 0}}),
    ] {
        let server = MockServer::start().await;
        let answer = chat_with(&server, ResponseTemplate::new(200).set_body_json(body))
            .await
            .unwrap();
        assert_eq!(answer.text, "");
        assert_eq!(answer.finish, Finish::Other);
        assert_eq!(
            answer.usage,
            Some(Usage {
                input_tokens: 12,
                output_tokens: 0
            })
        );
    }
}

/// A body that is not the protocol's shape stays `invalid_response`.
#[tokio::test]
async fn non_protocol_body_is_invalid_response() {
    for body in [
        json!({"object": "chat.completion"}),
        json!({"choices": {"0": "x"}}),
        json!([1, 2]),
    ] {
        let server = MockServer::start().await;
        let result = chat_with(&server, ResponseTemplate::new(200).set_body_json(body)).await;
        assert_eq!(reason(&result), FailureReason::InvalidResponse);
    }
    let server = MockServer::start().await;
    let result = chat_with(
        &server,
        ResponseTemplate::new(200).set_body_string("not json at all"),
    )
    .await;
    assert_eq!(reason(&result), FailureReason::InvalidResponse);
}

#[tokio::test]
async fn oversized_body_is_invalid_response() {
    let server = MockServer::start().await;
    let big = format!(
        "{{\"choices\":[{{\"message\":{{\"content\":\"{}\"}}}}]}}",
        "a".repeat(4 * 1024 * 1024)
    );
    let result = chat_with(
        &server,
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_string(big),
    )
    .await;
    assert_eq!(reason(&result), FailureReason::InvalidResponse);
}

/// A body sent in chunks with no `Content-Length` is cut at 4 MiB as it streams in.
#[tokio::test]
async fn oversized_chunked_body_is_invalid_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/v1", listener.local_addr().unwrap())).unwrap();
    let sent = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&sent);
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        read_request(&mut socket).await;
        let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                    transfer-encoding: chunked\r\n\r\n";
        if socket.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        let chunk = vec![b'a'; 64 * 1024];
        // 6 MiB in 64 KiB chunks; the client hangs up once it is past the cap.
        for _ in 0..96 {
            let frame = format!("{:x}\r\n", chunk.len());
            if socket.write_all(frame.as_bytes()).await.is_err()
                || socket.write_all(&chunk).await.is_err()
                || socket.write_all(b"\r\n").await.is_err()
            {
                return;
            }
            counted.fetch_add(chunk.len(), Ordering::SeqCst);
        }
        let _ = socket.write_all(b"0\r\n\r\n").await;
    });
    let provider = OpenAiCompatible::new(client(), base, secret(), &config(), None);
    let err = provider.chat(&text_request()).await.unwrap_err();
    assert_eq!(err.reason, FailureReason::InvalidResponse);
    assert!(
        sent.load(Ordering::SeqCst) < 6 * 1024 * 1024,
        "the read stopped before the end"
    );
}

#[tokio::test]
async fn redirect_is_invalid_response() {
    let target = MockServer::start().await;
    mount_ok(&target).await;
    let server = MockServer::start().await;
    let location = format!("{}/v1/chat/completions", target.uri());
    let result = chat_with(
        &server,
        ResponseTemplate::new(307).insert_header("location", location.as_str()),
    )
    .await;
    assert_eq!(reason(&result), FailureReason::InvalidResponse);
    assert!(target.received_requests().await.unwrap().is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn status_maps_to_reason() {
    let cases = [
        (401, FailureReason::KeyRejected),
        (403, FailureReason::KeyRejected),
        (404, FailureReason::ModelNotFound),
        (400, FailureReason::BadRequest),
        (422, FailureReason::BadRequest),
        (429, FailureReason::RateLimited),
        (500, FailureReason::Unavailable),
        (502, FailureReason::Unavailable),
    ];
    for (status, expected) in cases {
        let server = MockServer::start().await;
        let template = ResponseTemplate::new(status)
            .set_body_string("{\"error\":{\"message\":\"BODY-SECRET echo of PROMPT\"}}");
        let result = chat_with(&server, template).await;
        let err = result.unwrap_err();
        assert_eq!(err.reason, expected, "{status}");
        assert_eq!(err.retry_after, None, "{status}");
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains("BODY-SECRET"), "{shown}");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "{status} is not retried"
        );
    }
}

#[tokio::test]
async fn status_429_with_retry_after_retried_inside_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_ok(&server).await;
    let response = provider(&server).chat(&text_request()).await.unwrap();
    assert_eq!(response.text, "{\"variants\":[]}");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn status_503_with_retry_after_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_ok(&server).await;
    provider(&server).chat(&text_request()).await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn retries_stop_after_three_attempts() {
    let server = MockServer::start().await;
    let result = chat_with(
        &server,
        ResponseTemplate::new(503).insert_header("retry-after", "0"),
    )
    .await;
    let err = result.unwrap_err();
    assert_eq!(err.reason, FailureReason::Unavailable);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn status_429_past_deadline_returns_rate_limited_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "600"))
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = provider(&server)
        .chat(&request(OutputMode::Text, Duration::from_secs(2)))
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(1), "no sleep");
    assert_eq!(err.reason, FailureReason::RateLimited);
    assert_eq!(err.retry_after, Some(Duration::from_secs(600)));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// The wait a `Retry-After` asks for is weighed against the time left after the attempt, not
/// at the start: a slow `429` leaves too little time, so it answers `rate_limited` at once.
#[tokio::test]
async fn retry_after_weighed_against_time_left_after_a_slow_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "1")
                .set_delay(Duration::from_millis(1_500)),
        )
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = provider(&server)
        .chat(&request(OutputMode::Text, Duration::from_secs(2)))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::RateLimited);
    assert_eq!(err.retry_after, Some(Duration::from_secs(1)));
    assert!(started.elapsed() < Duration::from_millis(1_900), "no sleep");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// Reads one HTTP request (headers, then `content-length` bytes of body).
async fn read_request(socket: &mut tokio::net::TcpStream) {
    let mut read = Vec::new();
    let mut buffer = vec![0_u8; 8192];
    loop {
        let n = socket.read(&mut buffer).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        read.extend_from_slice(&buffer[..n]);
        let Some(end) = read.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&read[..end]).to_ascii_lowercase();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .map_or(0, |v| v.trim().parse().unwrap());
        if read.len() >= end + 4 + length {
            return;
        }
    }
}

#[tokio::test]
async fn lost_response_not_retried() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/v1", listener.local_addr().unwrap())).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&connections);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            seen.fetch_add(1, Ordering::SeqCst);
            read_request(&mut socket).await;
            let _ = socket.shutdown().await;
            drop(socket);
        }
    });
    let provider = OpenAiCompatible::new(client(), base, secret(), &config(), None);
    let err = provider.chat(&text_request()).await.unwrap_err();
    assert_eq!(err.reason, FailureReason::Unavailable);
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn slow_answer_hits_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_body().set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = provider(&server)
        .chat(&request(OutputMode::Text, Duration::from_millis(300)))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn elapsed_deadline_sends_nothing() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let err = provider(&server)
        .chat(&request(OutputMode::Text, Duration::ZERO))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::Timeout);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn models_lists_data_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-test-key"))
        .and(query_param("api-version", "2026-01-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"id": "gpt-a", "object": "model"},
                {"id": "vendor/b", "name": "Vendor B"},
                {"object": "model"}
            ]
        })))
        .mount(&server)
        .await;
    let models = provider(&server)
        .models(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["gpt-a", "vendor/b"]);
    assert_eq!(models[0].name, None);
    assert_eq!(models[1].name.as_deref(), Some("Vendor B"));
}

#[tokio::test]
async fn models_without_data_is_invalid_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
        .mount(&server)
        .await;
    let err = provider(&server)
        .models(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::InvalidResponse);
}

#[tokio::test]
async fn models_respect_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": []}))
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;
    let started = Instant::now();
    let err = provider(&server)
        .models(Instant::now() + Duration::from_millis(300))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn minimax_extra_body_and_openrouter_header_sent() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let minimax = OpenAiCompatible::new(
        client(),
        base(&server),
        secret(),
        &config(),
        preset("minimax"),
    );
    minimax.chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert_eq!(body["reasoning_split"], true);
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("x-openrouter-title").is_none());

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("x-openrouter-title", "postit"))
        .respond_with(ok_body())
        .mount(&server)
        .await;
    let openrouter = OpenAiCompatible::new(
        client(),
        base(&server),
        secret(),
        &config(),
        preset("openrouter"),
    );
    openrouter.chat(&text_request()).await.unwrap();
    let body = body_of(&server).await;
    assert!(body.get("reasoning_split").is_none(), "{body}");
}

#[tokio::test]
async fn no_key_sends_no_authorization() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let provider = OpenAiCompatible::new(
        client(),
        base(&server),
        ProviderSecret::default(),
        &ProviderConfig::default(),
        preset("ollama"),
    );
    provider.chat(&text_request()).await.unwrap();
    let requests: Vec<Request> = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn private_address_refused() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let strict = build_public_client(&settings(), AddressPolicy::strict()).unwrap();
    let base = Url::parse(&format!("http://127.0.0.1:{}/v1", server.address().port())).unwrap();
    let provider = OpenAiCompatible::new(strict, base, secret(), &config(), None);
    let err = provider.chat(&text_request()).await.unwrap_err();
    assert_eq!(err.reason, FailureReason::AddressRefused);
    let err = provider
        .models(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(err.reason, FailureReason::AddressRefused);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn chat_request_debug_hides_content() {
    let request = request(schema_mode(), Duration::from_secs(10));
    let shown = format!("{request:?}");
    assert!(!shown.contains("PROMPT-SECRET"), "{shown}");
    assert!(!shown.contains("bakery"), "{shown}");
    assert!(!shown.contains(&STANDARD.encode(IMAGE)), "{shown}");
    assert!(!shown.contains("JPEG-SECRET"), "{shown}");
}

#[tokio::test]
async fn provider_debug_hides_secrets() {
    let server_base = Url::parse("https://api.example.com/v1").unwrap();
    let provider = OpenAiCompatible::new(client(), server_base, secret(), &config(), None);
    let shown = format!("{provider:?}");
    assert!(!shown.contains("sk-test-key"), "{shown}");
    assert!(!shown.contains("org-1"), "{shown}");
}
