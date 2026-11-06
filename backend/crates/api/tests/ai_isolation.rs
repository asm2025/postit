//! The `/api/v1/ai` isolation matrix: the owner, a delegate at each level, a delegate limited to
//! other accounts, and an unrelated user, on every route.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[cfg(feature = "ai")]
mod with_ai {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use postit_api::testkit::{
        ADMIN_SUB, AI_TEXT_PLATFORM, TestApp, TestResponse, seed_ai_account,
    };
    use postit_core::{AccountId, UserId};
    use postit_data::testkit::grant;
    use postit_data::{AccountLimit, Level};
    use postit_drafter::testkit::{ScriptedFactory, ScriptedProvider};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use uuid::Uuid;

    const ACT_AS: &str = "x-postit-act-as";
    const OWNER_KEY: &str = "sk-iso-OWNER-KEY-0042";

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

    fn openai(key: &str) -> Value {
        json!({ "preset": "openai", "api_key": key, "default_model": "gpt-test" })
    }

    fn answer(targets: &[AccountId]) -> String {
        let variants: Vec<Value> = targets
            .iter()
            .map(|t| json!({ "target": t.to_string(), "text": "Spring menu is here" }))
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

    /// One route call: method, path, and body.
    struct Route {
        name: &'static str,
        method: Method,
        path: String,
        body: Option<Value>,
    }

    impl Route {
        fn new(name: &'static str, method: Method, path: String, body: Option<Value>) -> Self {
            Self {
                name,
                method,
                path,
                body,
            }
        }
    }

    /// Alice owns a provider and two accounts; `caller` acts in her workspace.
    struct World {
        app: TestApp,
        provider: Arc<ScriptedProvider>,
        pool: PgPool,
        alice: String,
        alice_id: UserId,
        provider_id: String,
        /// The provider as Alice saw it when she created it.
        created: Value,
        a1: AccountId,
        a2: AccountId,
    }

    impl World {
        async fn new(pool: PgPool) -> Self {
            let provider = Arc::new(ScriptedProvider::new());
            let factory = Arc::new(ScriptedFactory::new(Arc::clone(&provider)));
            let app = TestApp::start_with_drafter(pool.clone(), factory).await;
            let (alice, alice_id) = active_user(&app, "alice").await;
            let created = app
                .call(
                    Method::POST,
                    "/api/v1/ai/providers",
                    Some(&alice),
                    Some(openai(OWNER_KEY)),
                )
                .await;
            assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
            let provider_id = created.body["id"].as_str().unwrap().to_owned();
            let created = created.body;
            let a1 = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
            let a2 = seed_ai_account(&pool, alice_id, AI_TEXT_PLATFORM).await;
            Self {
                app,
                provider,
                pool,
                alice,
                alice_id,
                provider_id,
                created,
                a1,
                a2,
            }
        }

        fn provider_path(&self) -> String {
            format!("/api/v1/ai/providers/{}", self.provider_id)
        }

        /// The routes only the owner may call.
        fn owner_only_routes(&self) -> Vec<Route> {
            let one = self.provider_path();
            vec![
                Route::new(
                    "create",
                    Method::POST,
                    "/api/v1/ai/providers".to_owned(),
                    Some(openai("sk-iso-DELEGATE-KEY")),
                ),
                Route::new(
                    "update",
                    Method::PATCH,
                    one.clone(),
                    Some(json!({ "name": "Taken over", "make_default": true })),
                ),
                Route::new("delete", Method::DELETE, one.clone(), None),
                Route::new("test", Method::POST, format!("{one}/test"), None),
                Route::new(
                    "generations",
                    Method::GET,
                    "/api/v1/ai/generations".to_owned(),
                    None,
                ),
            ]
        }

        /// The routes that need `edit`.
        fn edit_routes(&self) -> Vec<Route> {
            let one = self.provider_path();
            vec![
                Route::new("list", Method::GET, "/api/v1/ai/providers".to_owned(), None),
                Route::new("get", Method::GET, one.clone(), None),
                Route::new("models", Method::GET, format!("{one}/models"), None),
                Route::new(
                    "generate",
                    Method::POST,
                    "/api/v1/ai/generate".to_owned(),
                    Some(generate_body(&[self.a1])),
                ),
            ]
        }

        async fn delegate(&self, sub: &str, level: Level, limit: AccountLimit) -> String {
            let (token, id) = active_user(&self.app, sub).await;
            grant(&self.pool, self.alice_id, id, level, limit).await;
            token
        }

        async fn as_alice(&self, token: &str, route: &Route) -> TestResponse {
            call_as(
                &self.app,
                route.method.clone(),
                &route.path,
                token,
                Some(self.alice_id),
                route.body.clone(),
            )
            .await
        }

        /// The provider is still there, unchanged, as Alice sees it.
        async fn provider_untouched(&self) {
            let res = self
                .app
                .call(Method::GET, &self.provider_path(), Some(&self.alice), None)
                .await;
            assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
            assert_eq!(res.body, self.created);
            let list = self
                .app
                .call(Method::GET, "/api/v1/ai/providers", Some(&self.alice), None)
                .await;
            assert_eq!(list.body.as_array().unwrap().len(), 1, "{:?}", list.body);
        }
    }

    /// A delegate's view of a provider: names and models, never secrets or configuration.
    fn assert_delegate_view(provider: &Value, what: &str) {
        assert_eq!(provider["view"], "delegate", "{what}: {provider:?}");
        let object = provider.as_object().unwrap();
        for hidden in ["base_url", "has_key", "header_names", "protocol", "api_key"] {
            assert!(!object.contains_key(hidden), "{what} shows {hidden}");
        }
        assert!(!provider.to_string().contains(OWNER_KEY), "{what}");
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn owner_allowed_on_every_route(pool: PgPool) {
        let w = World::new(pool).await;
        let alice = w.alice.as_str();
        let one = w.provider_path();

        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/presets", Some(alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "presets: {:?}", res.body);
        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/providers", Some(alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "list: {:?}", res.body);
        assert_eq!(res.body[0]["view"], "owner");
        let res = w.app.call(Method::GET, &one, Some(alice), None).await;
        assert_eq!(res.status, StatusCode::OK, "get: {:?}", res.body);
        assert_eq!(res.body["view"], "owner");
        let res = w
            .app
            .call(Method::GET, &format!("{one}/models"), Some(alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "models: {:?}", res.body);
        w.provider.push_text("OK");
        let res = w
            .app
            .call(Method::POST, &format!("{one}/test"), Some(alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "test: {:?}", res.body);
        assert_eq!(res.body["ok"], true);
        w.provider.push_text(answer(&[w.a1, w.a2]));
        let res = w
            .app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(alice),
                Some(generate_body(&[w.a1, w.a2])),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "generate: {:?}", res.body);
        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/generations", Some(alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "generations: {:?}", res.body);
        assert_eq!(res.body["total"], 1);
        let res = w
            .app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(alice),
                Some(openai("sk-iso-SECOND")),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "create: {:?}", res.body);
        let res = w
            .app
            .call(
                Method::PATCH,
                &one,
                Some(alice),
                Some(json!({ "name": "Renamed" })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "update: {:?}", res.body);
        let res = w.app.call(Method::DELETE, &one, Some(alice), None).await;
        assert_eq!(res.status, StatusCode::NO_CONTENT, "delete: {:?}", res.body);
        assert_eq!(w.provider.chat_calls(), 2);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn view_delegate_refused_everything_but_presets(pool: PgPool) {
        let w = World::new(pool).await;
        let viewer = w.delegate("viewer", Level::View, AccountLimit::All).await;

        let presets = Route::new(
            "presets",
            Method::GET,
            "/api/v1/ai/presets".to_owned(),
            None,
        );
        let res = w.as_alice(&viewer, &presets).await;
        assert_eq!(res.status, StatusCode::OK, "presets: {:?}", res.body);
        assert_eq!(res.body.as_array().unwrap().len(), 11);

        for route in w.edit_routes() {
            let res = w.as_alice(&viewer, &route).await;
            assert_code(
                &res,
                StatusCode::FORBIDDEN,
                "capability_required",
                route.name,
            );
        }
        // Below `edit` and also owner-only: a delegate is refused as one, at any level.
        for route in w.owner_only_routes() {
            let res = w.as_alice(&viewer, &route).await;
            assert_code(&res, StatusCode::FORBIDDEN, "owner_only", route.name);
        }
        assert_eq!(w.provider.chat_calls(), 0);
        assert_eq!(w.provider.model_calls(), 0);
        w.provider_untouched().await;
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn edit_and_publish_delegates_draft_but_never_manage(pool: PgPool) {
        let w = World::new(pool).await;
        for (sub, level) in [("editor", Level::Edit), ("publisher", Level::Publish)] {
            let token = w.delegate(sub, level, AccountLimit::All).await;
            let calls = w.provider.chat_calls();

            let list = w.edit_routes().remove(0);
            let res = w.as_alice(&token, &list).await;
            assert_eq!(res.status, StatusCode::OK, "{sub} list: {:?}", res.body);
            let providers = res.body.as_array().unwrap();
            assert_eq!(providers.len(), 1, "{sub}");
            assert_delegate_view(&providers[0], sub);
            assert_eq!(providers[0]["id"], w.provider_id.as_str());
            assert_eq!(providers[0]["is_default"], true);
            assert_eq!(providers[0]["default_model"], "gpt-test");

            let get = w.edit_routes().remove(1);
            let res = w.as_alice(&token, &get).await;
            assert_eq!(res.status, StatusCode::OK, "{sub} get: {:?}", res.body);
            assert_delegate_view(&res.body, sub);

            let models = w.edit_routes().remove(2);
            let res = w.as_alice(&token, &models).await;
            assert_eq!(res.status, StatusCode::OK, "{sub} models: {:?}", res.body);
            assert_eq!(res.body["models"][0]["id"], "gpt-test");

            w.provider.push_text(answer(&[w.a1]));
            let generate = w.edit_routes().remove(3);
            let res = w.as_alice(&token, &generate).await;
            assert_eq!(res.status, StatusCode::OK, "{sub} generate: {:?}", res.body);
            assert_eq!(res.body["provider_id"], w.provider_id.as_str());
            assert_eq!(
                res.body["draft"]["overrides"][w.a1.to_string()]["text"],
                "Spring menu is here"
            );

            for route in w.owner_only_routes() {
                let res = w.as_alice(&token, &route).await;
                assert_code(
                    &res,
                    StatusCode::FORBIDDEN,
                    "owner_only",
                    &format!("{sub} {}", route.name),
                );
            }
            assert_eq!(
                w.provider.chat_calls(),
                calls + 1,
                "{sub}: only the generate reached the provider"
            );
            w.provider_untouched().await;
        }

        // The delegates' generations are in Alice's history, under their names.
        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/generations", Some(&w.alice), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["total"], 2);
        let actors: Vec<&Value> = res.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| &row["actor"])
            .collect();
        assert!(
            actors.iter().all(|a| **a != w.alice_id.to_string()),
            "{actors:?}"
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn limited_delegate_gets_404_outside_the_limit(pool: PgPool) {
        let w = World::new(pool).await;
        let limited = w
            .delegate("limited", Level::Publish, AccountLimit::only([w.a2]))
            .await;

        for targets in [vec![w.a1], vec![w.a2, w.a1]] {
            let route = Route::new(
                "generate",
                Method::POST,
                "/api/v1/ai/generate".to_owned(),
                Some(generate_body(&targets)),
            );
            let res = w.as_alice(&limited, &route).await;
            assert_code(
                &res,
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("{targets:?}"),
            );
        }
        assert_eq!(w.provider.chat_calls(), 0, "refused before the provider");

        // Inside the limit the delegate drafts as any other.
        w.provider.push_text(answer(&[w.a2]));
        let route = Route::new(
            "generate",
            Method::POST,
            "/api/v1/ai/generate".to_owned(),
            Some(generate_body(&[w.a2])),
        );
        let res = w.as_alice(&limited, &route).await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        // The provider is the owner's, not tied to an account: the limit does not hide it.
        let res = w.as_alice(&limited, &w.edit_routes().remove(0)).await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_delegate_view(&res.body[0], "limited");
        for route in w.owner_only_routes() {
            let res = w.as_alice(&limited, &route).await;
            assert_code(&res, StatusCode::FORBIDDEN, "owner_only", route.name);
        }
        w.provider_untouched().await;
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn unrelated_user_sees_nothing_of_another_owner(pool: PgPool) {
        let w = World::new(pool.clone()).await;
        let (mallory, mallory_id) = active_user(&w.app, "mallory").await;
        let own = seed_ai_account(&pool, mallory_id, AI_TEXT_PLATFORM).await;
        let one = w.provider_path();

        for (method, path, body) in [
            (Method::GET, one.clone(), None),
            (
                Method::PATCH,
                one.clone(),
                Some(json!({ "name": "Mine now" })),
            ),
            (Method::DELETE, one.clone(), None),
            (Method::POST, format!("{one}/test"), None),
            (Method::GET, format!("{one}/models"), None),
        ] {
            let res = w
                .app
                .call(method.clone(), &path, Some(&mallory), body)
                .await;
            assert_code(
                &res,
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("{method} {path}"),
            );
        }
        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/providers", Some(&mallory), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body, json!([]));
        let res = w
            .app
            .call(Method::GET, "/api/v1/ai/generations", Some(&mallory), None)
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["total"], 0);

        // Another owner's provider ID is no provider of Mallory's.
        let mut body = generate_body(&[own]);
        body["provider_id"] = json!(w.provider_id);
        let res = w
            .app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&mallory),
                Some(body),
            )
            .await;
        assert_code(
            &res,
            StatusCode::CONFLICT,
            "ai_provider_required",
            "another owner's provider",
        );
        // With a provider of her own, Alice's accounts are still not hers to draft for.
        let res = w
            .app
            .call(
                Method::POST,
                "/api/v1/ai/providers",
                Some(&mallory),
                Some(openai("sk-iso-MALLORY")),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        let res = w
            .app
            .call(
                Method::POST,
                "/api/v1/ai/generate",
                Some(&mallory),
                Some(generate_body(&[w.a1])),
            )
            .await;
        assert_code(
            &res,
            StatusCode::NOT_FOUND,
            "not_found",
            "another owner's account",
        );
        // Naming Alice's workspace with no grant is refused outright.
        let res = call_as(
            &w.app,
            Method::GET,
            "/api/v1/ai/providers",
            &mallory,
            Some(w.alice_id),
            None,
        )
        .await;
        assert_code(
            &res,
            StatusCode::FORBIDDEN,
            "delegation_invalid",
            "act as without a grant",
        );
        assert_eq!(w.provider.chat_calls(), 0);
        assert_eq!(w.provider.model_calls(), 0);
        w.provider_untouched().await;
    }
}
