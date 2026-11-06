//! The `/api/v1/ai` routes: presets, provider management, the connection test, models,
//! generation, and the owner's generation history. The isolation matrix and the leak checks
//! live in `ai_isolation.rs` and `ai_leak.rs`.
#![allow(clippy::unwrap_used)]

#[cfg(feature = "ai")]
mod with_ai {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use postit_api::testkit::{
        ADMIN_SUB, AI_TEXT_PLATFORM, DrafterTestOptions, TestApp, TestResponse, seed_ai_account,
    };
    use postit_config::RateBucket;
    use postit_core::{AccountId, UserId};
    use postit_data::testkit::grant;
    use postit_data::{AccountLimit, Level};
    use postit_drafter::testkit::{ScriptedFactory, ScriptedProvider};
    use postit_drafter::{ChatResponse, FailureReason, Finish, LlmError};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use uuid::Uuid;

    const ACT_AS: &str = "x-postit-act-as";

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

    async fn call_as(
        app: &TestApp,
        method: Method,
        path: &str,
        token: &str,
        owner: Option<UserId>,
        body: Option<Value>,
    ) -> TestResponse {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(owner) = owner {
            b = b.header(ACT_AS, owner.to_string());
        }
        let body = match body {
            Some(json) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        app.send(b.body(body).unwrap()).await
    }

    fn assert_code(res: &TestResponse, status: StatusCode, code: &str, what: &str) {
        assert_eq!(res.status, status, "{what}: {:?}", res.body);
        assert_eq!(res.body["code"], code, "{what}");
    }

    fn scripted() -> (Arc<ScriptedProvider>, Arc<ScriptedFactory>) {
        let provider = Arc::new(ScriptedProvider::new());
        let factory = Arc::new(ScriptedFactory::new(Arc::clone(&provider)));
        (provider, factory)
    }

    async fn app_with(pool: PgPool) -> (TestApp, Arc<ScriptedProvider>) {
        let (provider, factory) = scripted();
        (TestApp::start_with_drafter(pool, factory).await, provider)
    }

    fn openai(key: &str) -> Value {
        json!({ "preset": "openai", "api_key": key, "default_model": "gpt-test" })
    }

    async fn create(app: &TestApp, token: &str, body: Value) -> TestResponse {
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(token),
                Some(body),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        res
    }

    fn id_of(res: &TestResponse) -> String {
        res.body["id"].as_str().unwrap().to_owned()
    }

    /// A valid answer with one short variant per target.
    fn answer(targets: &[AccountId]) -> String {
        let variants: Vec<Value> = targets
            .iter()
            .map(|t| json!({ "target": t.to_string(), "text": "Spring menu is here", "tags": ["food"] }))
            .collect();
        json!({ "variants": variants }).to_string()
    }

    fn generate_body(targets: &[AccountId]) -> Value {
        json!({
            "mode": "post",
            "prompt": "Announce the spring menu",
            "targets": targets.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })
    }

    async fn generate(app: &TestApp, token: &str, targets: &[AccountId]) -> TestResponse {
        app.call(
            Method::POST,
            "/api/v1/ai/generate",
            Some(token),
            Some(generate_body(targets)),
        )
        .await
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn presets_listed_with_availability(pool: PgPool) {
        let (app, _) = app_with(pool).await;
        let (alice, _) = active_user(&app, "alice").await;
        let res = app
            .call(Method::GET, "/api/v1/ai/presets", Some(&alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        let presets = res.body.as_array().unwrap();
        assert_eq!(presets.len(), 11);
        let find = |key: &str| presets.iter().find(|p| p["key"] == key).unwrap().clone();
        let openai = find("openai");
        assert_eq!(openai["protocol"], "openai-compatible");
        assert_eq!(openai["local"], false);
        assert_eq!(openai["available"], true);
        assert_eq!(openai["subscription_note"], true);
        assert!(
            openai["guide_url"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert_eq!(find("anthropic")["protocol"], "anthropic");
        let ollama = find("ollama");
        assert_eq!(ollama["local"], true);
        assert_eq!(ollama["available"], false, "private endpoints are off");
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn local_presets_available_with_private_endpoints(pool: PgPool) {
        let (_, factory) = scripted();
        let open = TestApp::start_with_drafter_using(
            pool,
            DrafterTestOptions {
                factory,
                allow_private: true,
            },
            |_| {},
        )
        .await;
        let res = open
            .call(
                Method::GET,
                "/api/v1/ai/presets",
                Some(&open.token(ADMIN_SUB)),
                None,
            )
            .await;
        let ollama = res
            .body
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["key"] == "ollama")
            .unwrap()
            .clone();
        assert_eq!(ollama["available"], true);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn owner_crud_round_trip(pool: PgPool) {
        let (app, _) = app_with(pool).await;
        let (alice, _) = active_user(&app, "alice").await;
        let created = create(&app, &alice, openai("sk-test-1")).await;
        let id = id_of(&created);
        let location = format!("/api/v1/ai/providers/{id}");
        assert_eq!(
            created
                .headers
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok()),
            Some(location.as_str())
        );
        assert_eq!(created.body["view"], "owner");
        assert_eq!(created.body["preset"], "openai");
        assert_eq!(created.body["has_key"], true);
        assert_eq!(
            created.body["is_default"], true,
            "the first provider is the default"
        );
        assert_eq!(created.body["default_model"], "gpt-test");

        let list = app
            .call(Method::GET, "/api/v1/ai/providers", Some(&alice), None)
            .await;
        assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
        assert_eq!(list.body.as_array().unwrap().len(), 1);
        assert_eq!(list.body[0]["id"], id.as_str());

        let renamed = app
            .call(
                Method::PATCH,
                &location,
                Some(&alice),
                Some(json!({ "name": "Work OpenAI" })),
            )
            .await;
        assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
        assert_eq!(renamed.body["name"], "Work OpenAI");

        let deleted = app
            .call(Method::DELETE, &location, Some(&alice), None)
            .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
        let again = app
            .call(
                Method::PATCH,
                &location,
                Some(&alice),
                Some(json!({ "name": "x" })),
            )
            .await;
        assert_code(
            &again,
            StatusCode::NOT_FOUND,
            "not_found",
            "patch after delete",
        );
        let again = app
            .call(Method::DELETE, &location, Some(&alice), None)
            .await;
        assert_code(
            &again,
            StatusCode::NOT_FOUND,
            "not_found",
            "delete after delete",
        );
        let list = app
            .call(Method::GET, "/api/v1/ai/providers", Some(&alice), None)
            .await;
        assert_eq!(list.body.as_array().unwrap().len(), 0);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn create_needs_exactly_one_source(pool: PgPool) {
        let (app, _) = app_with(pool).await;
        let (alice, _) = active_user(&app, "alice").await;
        for body in [
            json!({}),
            json!({ "preset": "openai", "api_key": "k", "opencode": { "npm": "@ai-sdk/openai" } }),
        ] {
            let res = app
                .call(
                    Method::POST,
                    "/api/v1/ai/providers",
                    Some(&alice),
                    Some(body),
                )
                .await;
            assert_code(
                &res,
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "source",
            );
        }
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(&alice),
                Some(json!({ "preset": "no-such-preset", "api_key": "k" })),
            )
            .await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "preset",
        );
        assert!(
            res.body["violations"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn api_key_never_returned(pool: PgPool) {
        let (app, _) = app_with(pool).await;
        let (alice, _) = active_user(&app, "alice").await;
        let secrets = [
            "sk-SECRET-KEY-123",
            "sk-PASTED-456",
            "hv-HEADER-789",
            "sk-NEW-000",
            "hv-NEW-111",
        ];
        let mut bodies = Vec::new();
        let preset = create(&app, &alice, openai(secrets[0])).await;
        bodies.push(preset.bytes.clone());
        let pasted = create(
            &app,
            &alice,
            json!({ "opencode": {
                "provider": { "mine": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "My gateway",
                    "options": {
                        "baseURL": "https://gateway.example.com/v1",
                        "apiKey": secrets[1],
                        "headers": { "X-Org": secrets[2] }
                    },
                    "models": { "m1": { "name": "Model one" } }
                } }
            } }),
        )
        .await;
        bodies.push(pasted.bytes.clone());
        assert_eq!(pasted.body["view"], "owner");
        assert_eq!(pasted.body["has_key"], true);
        assert_eq!(pasted.body["header_names"], json!(["X-Org"]));
        let patched = app
            .call(
                Method::PATCH,
                &format!("/api/v1/ai/providers/{}", id_of(&pasted)),
                Some(&alice),
                Some(json!({ "api_key": secrets[3], "headers": { "X-Org": secrets[4] } })),
            )
            .await;
        assert_eq!(patched.status, StatusCode::OK, "{:?}", patched.body);
        bodies.push(patched.bytes.clone());
        let list = app
            .call(Method::GET, "/api/v1/ai/providers", Some(&alice), None)
            .await;
        assert_eq!(list.status, StatusCode::OK);
        bodies.push(list.bytes.clone());
        for body in &bodies {
            let text = String::from_utf8_lossy(body);
            for secret in secrets {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
            assert!(!text.contains("api_key\""), "no api_key member: {text}");
        }
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn generate_returns_draft(pool: PgPool) {
        let (app, provider) = app_with(pool.clone()).await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        let created = create(&app, &alice, openai("sk-test-1")).await;
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        let b = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        provider.push_text(answer(&[a, b]));

        let res = generate(&app, &alice, &[a, b]).await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert!(Uuid::parse_str(res.body["generation_id"].as_str().unwrap()).is_ok());
        assert_eq!(res.body["provider_id"], created.body["id"]);
        assert_eq!(res.body["model"], "gpt-test");
        let draft = &res.body["draft"];
        assert_eq!(draft["post"]["disclose_ai_generated"], true);
        assert_eq!(draft["post"]["text"], "Spring menu is here");
        let overrides = draft["overrides"].as_object().unwrap();
        assert_eq!(overrides.len(), 2, "one override per target");
        for target in [a, b] {
            assert_eq!(
                overrides[&target.to_string()]["text"],
                "Spring menu is here",
                "{target}"
            );
        }
        assert_eq!(res.body["violations"], json!({}));
        assert_eq!(res.body["notes"], json!([]));
        assert_eq!(
            res.body["usage"],
            json!({ "input_tokens": 100, "output_tokens": 20 })
        );
        assert_eq!(provider.chat_calls(), 1);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn generate_without_provider_409_ai_provider_required(pool: PgPool) {
        let (app, provider) = app_with(pool.clone()).await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        let res = generate(&app, &alice, &[a]).await;
        assert_code(
            &res,
            StatusCode::CONFLICT,
            "ai_provider_required",
            "no provider",
        );
        let mut body = generate_body(&[a]);
        body["provider_id"] = json!(Uuid::now_v7().to_string());
        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&alice),
                Some(body),
            )
            .await;
        assert_code(
            &res,
            StatusCode::CONFLICT,
            "ai_provider_required",
            "a provider that is gone",
        );
        assert_eq!(provider.chat_calls(), 0);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn provider_failure_502_with_reason_and_retry_after(pool: PgPool) {
        let (app, provider) = app_with(pool.clone()).await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        create(&app, &alice, openai("sk-test-1")).await;
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;

        provider.push(Err(LlmError {
            reason: FailureReason::RateLimited,
            retry_after: Some(Duration::from_secs(7)),
        }));
        let res = generate(&app, &alice, &[a]).await;
        assert_code(&res, StatusCode::BAD_GATEWAY, "ai_provider_failed", "429");
        assert_eq!(res.body["detail"], "rate_limited");
        assert_eq!(
            res.headers
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("7")
        );

        provider.push(Err(LlmError {
            reason: FailureReason::Unavailable,
            retry_after: Some(Duration::from_secs(3)),
        }));
        let res = generate(&app, &alice, &[a]).await;
        assert_code(&res, StatusCode::BAD_GATEWAY, "ai_provider_failed", "503");
        assert_eq!(res.body["detail"], "unavailable");
        assert!(
            res.headers.get(header::RETRY_AFTER).is_none(),
            "Retry-After only for rate_limited"
        );

        provider.push(Err(LlmError::new(FailureReason::KeyRejected)));
        let res = generate(&app, &alice, &[a]).await;
        assert_code(&res, StatusCode::BAD_GATEWAY, "ai_provider_failed", "401");
        assert_eq!(res.body["detail"], "key_rejected");
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn generate_validation_422(pool: PgPool) {
        let (app, provider) = app_with(pool.clone()).await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        create(&app, &alice, openai("sk-test-1")).await;
        let mut targets = Vec::new();
        for _ in 0..21 {
            targets.push(seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await);
        }
        let res = generate(&app, &alice, &targets).await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "21 targets",
        );
        let violations = res.body["violations"].as_array().unwrap();
        assert!(!violations.is_empty());
        assert_eq!(violations[0]["path"], "targets");
        assert_eq!(provider.chat_calls(), 0);

        let res = app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&alice),
                Some(json!({ "mode": "shout", "prompt": "x", "targets": [] })),
            )
            .await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "unknown mode",
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn test_and_models_answer(pool: PgPool) {
        let (app, provider) = app_with(pool).await;
        let (alice, _) = active_user(&app, "alice").await;
        let id = id_of(&create(&app, &alice, openai("sk-test-1")).await);
        provider.push_text("OK");
        let res = app
            .call(
                Method::POST,
                &format!("/api/v1/ai/providers/{id}/test"),
                Some(&alice),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["ok"], true);
        assert_eq!(res.body["models_found"], 0);

        provider.push(Err(LlmError::new(FailureReason::KeyRejected)));
        let res = app
            .call(
                Method::POST,
                &format!("/api/v1/ai/providers/{id}/test"),
                Some(&alice),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["ok"], false);
        assert_eq!(res.body["reason"], "key_rejected");

        let res = app
            .call(
                Method::GET,
                &format!("/api/v1/ai/providers/{id}/models"),
                Some(&alice),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["models"][0]["id"], "gpt-test");
        assert_eq!(res.body["models"][0]["source"], "config");
        assert!(res.body["remote_error"].is_null());

        // MiniMax lists no models, so with no default model there is nothing to test.
        let minimax = id_of(
            &create(
                &app,
                &alice,
                json!({ "preset": "minimax", "api_key": "sk-mm" }),
            )
            .await,
        );
        let res = app
            .call(
                Method::POST,
                &format!("/api/v1/ai/providers/{minimax}/test"),
                Some(&alice),
                None,
            )
            .await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "no model to test",
        );
        assert_eq!(res.body["violations"][0]["path"], "default_model");
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn rate_limited_after_burst(pool: PgPool) {
        let (provider, factory) = scripted();
        let app = TestApp::start_with_drafter_using(
            pool.clone(),
            DrafterTestOptions {
                factory,
                allow_private: false,
            },
            |s| {
                s.rate_limit.ai_requests = RateBucket {
                    rate_per_minute: 1,
                    burst: 5,
                };
            },
        )
        .await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        let id = id_of(&create(&app, &alice, openai("sk-test-1")).await);
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        let test_path = format!("/api/v1/ai/providers/{id}/test");
        let models_path = format!("/api/v1/ai/providers/{id}/models");

        for _ in 0..3 {
            provider.push_text(answer(&[a]));
            let res = generate(&app, &alice, &[a]).await;
            assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        }
        let res = app
            .call(Method::GET, &models_path, Some(&alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "models: {:?}", res.body);
        provider.push_text("OK");
        let res = app.call(Method::POST, &test_path, Some(&alice), None).await;
        assert_eq!(res.status, StatusCode::OK, "test: {:?}", res.body);

        let res = generate(&app, &alice, &[a]).await;
        assert_code(&res, StatusCode::TOO_MANY_REQUESTS, "rate_limited", "sixth");
        assert!(res.headers.contains_key(header::RETRY_AFTER));
        let res = app
            .call(Method::GET, &models_path, Some(&alice), None)
            .await;
        assert_code(
            &res,
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "models",
        );
        let res = app.call(Method::POST, &test_path, Some(&alice), None).await;
        assert_code(&res, StatusCode::TOO_MANY_REQUESTS, "rate_limited", "test");
        assert_eq!(
            provider.chat_calls(),
            4,
            "refused calls never reach the provider"
        );

        // Listing providers does not draw from the AI bucket.
        let res = app
            .call(Method::GET, "/api/v1/ai/providers", Some(&alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK);
        // Another user has a bucket of their own.
        let (bob, bob_id) = active_user(&app, "bob").await;
        create(&app, &bob, openai("sk-test-2")).await;
        let b = seed_ai_account(&pool, bob_id, AI_TEXT_PLATFORM).await;
        provider.push_text(answer(&[b]));
        assert_eq!(generate(&app, &bob, &[b]).await.status, StatusCode::OK);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn generate_route_outlives_global_timeout(pool: PgPool) {
        let (provider, factory) = scripted();
        let app = TestApp::start_with_drafter_using(
            pool.clone(),
            DrafterTestOptions {
                factory,
                allow_private: false,
            },
            |s| s.request_timeout = Duration::from_secs(1),
        )
        .await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        create(&app, &alice, openai("sk-test-1")).await;
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        provider.push_after(
            Duration::from_secs(2),
            Ok(ChatResponse {
                text: answer(&[a]),
                usage: None,
                finish: Finish::Stop,
            }),
        );
        let res = generate(&app, &alice, &[a]).await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert!(res.body["usage"].is_null(), "no usage reported");
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn generations_page_owner_only(pool: PgPool) {
        let (app, provider) = app_with(pool.clone()).await;
        let (alice, alice_id) = active_user(&app, "alice").await;
        let (bob, bob_id) = active_user(&app, "bob").await;
        let created = create(&app, &alice, openai("sk-test-1")).await;
        let a = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
        for _ in 0..2 {
            provider.push_text(answer(&[a]));
            assert_eq!(generate(&app, &alice, &[a]).await.status, StatusCode::OK);
        }

        let res = app
            .call(
                Method::GET,
                "/api/v1/ai/generations?page=1&page_size=1",
                Some(&alice),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["total"], 2);
        assert_eq!(res.body["page"], 1);
        assert_eq!(res.body["page_size"], 1);
        let rows = res.body["data"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["mode"], "post");
        assert_eq!(row["outcome"], "ok");
        assert_eq!(row["model"], "gpt-test");
        assert_eq!(row["provider_id"], created.body["id"]);
        assert_eq!(row["provider_name"], created.body["name"]);
        assert_eq!(row["actor"], alice_id.to_string());
        assert_eq!(row["input_tokens"], 100);
        assert_eq!(row["output_tokens"], 20);
        assert!(row["prompt"].is_null(), "prompts are not kept by default");
        assert!(row["post_id"].is_null());

        let res = app
            .call(
                Method::GET,
                "/api/v1/ai/generations?page_size=101",
                Some(&alice),
                None,
            )
            .await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "paging",
        );

        grant(&pool, alice_id, bob_id, Level::Publish, AccountLimit::All).await;
        let res = call_as(
            &app,
            Method::GET,
            "/api/v1/ai/generations",
            &bob,
            Some(alice_id),
            None,
        )
        .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", "delegate");
        let own = app
            .call(Method::GET, "/api/v1/ai/generations", Some(&bob), None)
            .await;
        assert_eq!(own.status, StatusCode::OK);
        assert_eq!(own.body["total"], 0);
    }
}

#[cfg(not(feature = "ai"))]
#[sqlx::test(migrations = "../data/migrations")]
async fn routes_absent_without_ai_feature(pool: sqlx::PgPool) {
    use axum::http::{Method, StatusCode};
    use postit_api::testkit::{ADMIN_SUB, TestApp};

    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    for (method, path) in [
        (Method::GET, "/api/v1/ai/presets"),
        (Method::GET, "/api/v1/ai/providers"),
        (Method::POST, "/api/v1/ai/generate"),
    ] {
        let res = app.call(method, path, Some(&token), None).await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}: {:?}", res.body);
        assert_eq!(res.body["code"], "not_found");
    }
}
