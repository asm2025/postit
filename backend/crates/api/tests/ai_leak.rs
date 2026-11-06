//! No AI key, secret header value, or prompt reaches a response, a log line, an audit row, or a
//! generation row, on any `/api/v1/ai` route, with the real OpenAI-compatible protocol talking to
//! a provider that echoes the request back in its errors. Its own test binary: tracing's
//! callsite-interest cache is process-wide, so a log-capturing test cannot share a binary with
//! others.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[cfg(feature = "ai")]
mod with_ai {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::{Method, StatusCode};
    use postit_api::testkit::{
        ADMIN_SUB, AI_TEXT_PLATFORM, DrafterTestOptions, TestApp, TestResponse, seed_ai_account,
    };
    use postit_config::HttpSettings;
    use postit_core::{AccountId, UserId};
    use postit_drafter::HttpProviderFactory;
    use postit_http::testkit::leak::{LogCapture, assert_no_leak};
    use postit_http::{AddressPolicy, build_public_client};
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    const KEY: &str = "sk-test-LEAK-ME-0123456789";
    const HEADER: &str = "hdr-LEAK-ME";
    const PROMPT: &str = "PROMPT-LEAK-ME";
    /// A prompt sent once the owner keeps prompts: it may be in the generation history and
    /// the generation rows, and nowhere else.
    const KEPT_PROMPT: &str = "KEPT-REQUEST-LEAK-ME";
    /// Opens every echoed error page, so a quoted page is found even without a secret in it.
    const ECHO_MARKER: &str = "ECHO-PAGE-LEAK-ME";

    async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
        let token = app.token(sub);
        let me = app
            .call(Method::GET, "/api/v1/me", Some(&token), None)
            .await;
        let id = Uuid::parse_str(me.body["id"].as_str().unwrap()).unwrap();
        let approved = app
            .call(
                Method::PATCH,
                &format!("/api/v1/users/{id}"),
                Some(&app.token(ADMIN_SUB)),
                Some(json!({ "status": "active" })),
            )
            .await;
        assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
        (token, UserId::from(id))
    }

    /// Answers with `status` and a body quoting every header and the whole request body, as a
    /// careless provider's error page might.
    struct Echo(u16);

    impl Respond for Echo {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let headers: Vec<String> = request
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap_or_default()))
                .collect();
            let echo = format!(
                "{ECHO_MARKER}\n{} {}\n{}\n\n{}",
                request.method,
                request.url,
                headers.join("\n"),
                String::from_utf8_lossy(&request.body)
            );
            ResponseTemplate::new(self.0)
                .set_body_json(json!({ "error": { "message": echo, "type": "echo" } }))
        }
    }

    fn completion(text: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": text },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 7 }
        }))
    }

    fn model_list() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({ "object": "list", "data": [{ "id": "m1", "object": "model" }] }))
    }

    /// The provider answers chats with `chat` and model lists with `models`, from now on.
    async fn answer_with(
        server: &MockServer,
        chat: impl Respond + 'static,
        models: impl Respond + 'static,
    ) {
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(chat)
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(models)
            .mount(server)
            .await;
    }

    fn answer(target: AccountId) -> String {
        json!({ "variants": [{ "target": target.to_string(), "text": "Spring menu is here" }] })
            .to_string()
    }

    fn client() -> postit_http::PublicClient {
        build_public_client(
            &HttpSettings {
                connect_timeout: Duration::from_secs(5),
                request_timeout: Duration::from_secs(30),
                user_agent: "postit-api-leak-test/0".to_owned(),
                extra_ca_files: Vec::<PathBuf>::new(),
            },
            AddressPolicy {
                allow_private: true,
            },
        )
        .unwrap()
    }

    async fn rows(pool: &PgPool, sql: &'static str) -> Vec<String> {
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn ai_leak(pool: PgPool) {
        let server = MockServer::start().await;
        let app = TestApp::start_with_drafter_using(
            pool.clone(),
            DrafterTestOptions {
                factory: Arc::new(HttpProviderFactory::new(client())),
                allow_private: true,
            },
            |_| {},
        )
        .await;
        let (owner, owner_id) = active_user(&app, "owner").await;
        let account = seed_ai_account(&pool, owner_id, AI_TEXT_PLATFORM).await;
        let prefs = app
            .call(Method::GET, "/api/v1/me/preferences", Some(&owner), None)
            .await;
        assert_eq!(prefs.status, StatusCode::OK, "{:?}", prefs.body);
        assert_eq!(prefs.body["store_ai_prompts"], false, "off by default");
        let (logs, guard) = LogCapture::install();

        let mut responses: Vec<(&str, TestResponse)> = Vec::new();
        let created = app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(&owner),
                Some(json!({ "opencode": { "provider": { "leaky": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "Leaky gateway",
                    "default_model": "m1",
                    "options": {
                        "baseURL": format!("{}/v1", server.uri()),
                        "apiKey": KEY,
                        "headers": { "X-Secret": HEADER }
                    },
                    "models": { "m1": { "name": "Model one" } }
                } } } })),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
        let id = created.body["id"].as_str().unwrap().to_owned();
        let one = format!("/api/v1/ai/providers/{id}");
        responses.push(("create", created));
        // A pasted entry naming the server's environment is refused, quoting nothing back.
        let refused = app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(&owner),
                Some(json!({ "opencode": { "provider": { "env": {
                    "npm": "@ai-sdk/openai-compatible",
                    "options": {
                        "baseURL": format!("{}/v1", server.uri()),
                        "apiKey": "{env:OPENAI_API_KEY}",
                        "headers": { "X-Secret": HEADER }
                    }
                } } } })),
            )
            .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{:?}",
            refused.body
        );
        responses.push(("refused create", refused));
        // The same key and header again: still a key change. The service never opens the stored
        // secret to compare it, so a `PATCH` carrying a key is always recorded as one, and the
        // secret is sealed afresh.
        let rekeyed = app
            .call(
                Method::PATCH,
                &one,
                Some(&owner),
                Some(json!({ "api_key": KEY, "headers": { "X-Secret": HEADER } })),
            )
            .await;
        assert_eq!(rekeyed.status, StatusCode::OK, "{:?}", rekeyed.body);
        responses.push(("rekey", rekeyed));
        let list = app
            .call(Method::GET, "/api/v1/ai/providers", Some(&owner), None)
            .await;
        assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
        responses.push(("list", list));
        let read = app.call(Method::GET, &one, Some(&owner), None).await;
        assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
        responses.push(("get", read));

        // The test, failing: the provider refuses the key and quotes the request.
        answer_with(&server, Echo(401), Echo(401)).await;
        let res = app
            .call(Method::POST, &format!("{one}/test"), Some(&owner), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["ok"], false);
        assert_eq!(res.body["reason"], "key_rejected");
        responses.push(("failing test", res));
        let res = app
            .call(Method::GET, &format!("{one}/models"), Some(&owner), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["remote_error"], "key_rejected");
        responses.push(("failing models", res));
        // The provider was sent the secrets, so its echo held them.
        let sent = server.received_requests().await.unwrap();
        assert!(!sent.is_empty());
        assert!(sent.iter().all(|r| {
            r.headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some(format!("Bearer {KEY}").as_str())
                && r.headers.get("x-secret").and_then(|v| v.to_str().ok()) == Some(HEADER)
        }));

        // The test, passing.
        answer_with(&server, completion("OK"), model_list()).await;
        let res = app
            .call(Method::POST, &format!("{one}/test"), Some(&owner), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["ok"], true, "{:?}", res.body);
        responses.push(("passing test", res));
        let res = app
            .call(Method::GET, &format!("{one}/models"), Some(&owner), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        responses.push(("passing models", res));

        let generate = json!({
            "mode": "post",
            "prompt": format!("{PROMPT}: announce the spring menu"),
            "targets": [account.to_string()],
        });
        // Generate, passing.
        answer_with(&server, completion(&answer(account)), model_list()).await;
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&owner),
                Some(generate.clone()),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        responses.push(("passing generate", res));
        let sent = server.received_requests().await.unwrap();
        assert!(
            sent.iter()
                .any(|r| String::from_utf8_lossy(&r.body).contains(PROMPT)),
            "the prompt went to the provider"
        );
        // Generate, the provider refusing the key and quoting the request.
        answer_with(&server, Echo(401), model_list()).await;
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&owner),
                Some(generate.clone()),
            )
            .await;
        assert_eq!(res.status, StatusCode::BAD_GATEWAY, "{:?}", res.body);
        assert_eq!(res.body["detail"], "key_rejected");
        responses.push(("generate 401", res));
        // Generate, the provider failing and quoting the request.
        answer_with(&server, Echo(500), model_list()).await;
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&owner),
                Some(generate),
            )
            .await;
        assert_eq!(res.status, StatusCode::BAD_GATEWAY, "{:?}", res.body);
        assert_eq!(res.body["detail"], "unavailable");
        responses.push(("generate 500", res));

        let res = app
            .call(Method::GET, "/api/v1/ai/generations", Some(&owner), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert!(res.body["total"].as_u64().unwrap() >= 1, "{:?}", res.body);
        assert!(
            res.body["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|g| g["prompt"].is_null()),
            "prompts are not kept"
        );
        responses.push(("generations", res));

        // The owner turns prompt storage on: the next prompt is kept, in the history only.
        let res = app
            .call(
                Method::PUT,
                "/api/v1/me/preferences",
                Some(&owner),
                Some(json!({ "store_ai_prompts": true })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["store_ai_prompts"], true);
        responses.push(("preferences", res));
        answer_with(&server, completion(&answer(account)), model_list()).await;
        let kept = format!("{KEPT_PROMPT}: announce the summer menu");
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&owner),
                Some(json!({
                    "mode": "post",
                    "prompt": kept,
                    "targets": [account.to_string()],
                })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        responses.push(("kept generate", res));
        let history = app
            .call(Method::GET, "/api/v1/ai/generations", Some(&owner), None)
            .await;
        assert_eq!(history.status, StatusCode::OK, "{:?}", history.body);
        let prompts: Vec<&serde_json::Value> = history.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| &g["prompt"])
            .collect();
        assert_eq!(prompts[0], &json!(kept), "the newest keeps its prompt");
        assert!(prompts[1..].iter().all(|p| p.is_null()), "{prompts:?}");

        let res = app.call(Method::DELETE, &one, Some(&owner), None).await;
        assert_eq!(res.status, StatusCode::NO_CONTENT, "{:?}", res.body);
        drop(guard);

        let audit = rows(
            &pool,
            "SELECT row_to_json(a)::text FROM audit_events a WHERE a.kind LIKE 'ai_%' ORDER BY a.at",
        )
        .await;
        for kind in [
            "ai_provider_created",
            "ai_provider_key_changed",
            "ai_provider_deleted",
        ] {
            assert!(
                audit.iter().any(|row| row.contains(kind)),
                "{kind} recorded: {audit:?}"
            );
        }
        let generations = rows(
            &pool,
            "SELECT row_to_json(g)::text FROM ai_generations g ORDER BY g.created_at",
        )
        .await;
        assert!(!generations.is_empty());
        assert_eq!(
            generations
                .iter()
                .filter(|row| row.contains(KEPT_PROMPT))
                .count(),
            1,
            "only the generation made with storage on keeps its prompt"
        );

        let text = logs.text();
        assert!(
            text.contains("/api/v1/ai/generate"),
            "the requests were logged"
        );
        let bodies: Vec<(String, String)> = responses
            .iter()
            .map(|(label, res)| {
                (
                    format!("{label} response"),
                    String::from_utf8_lossy(&res.bytes).into_owned(),
                )
            })
            .collect();
        let mut outputs: Vec<(&str, &str)> = bodies
            .iter()
            .map(|(label, body)| (label.as_str(), body.as_str()))
            .collect();
        outputs.push(("logs", text.as_str()));
        outputs.extend(audit.iter().map(|row| ("audit row", row.as_str())));
        assert_no_leak(
            &[
                KEY,
                HEADER,
                PROMPT,
                KEPT_PROMPT,
                ECHO_MARKER,
                owner.as_str(),
            ],
            &outputs,
        );
        // The history and the rows may hold the kept prompt, and nothing else.
        let history = String::from_utf8_lossy(&history.bytes).into_owned();
        let mut kept_in: Vec<(&str, &str)> = vec![("kept generations response", history.as_str())];
        kept_in.extend(
            generations
                .iter()
                .map(|row| ("generation row", row.as_str())),
        );
        assert_no_leak(
            &[KEY, HEADER, PROMPT, ECHO_MARKER, owner.as_str()],
            &kept_in,
        );
    }
}
