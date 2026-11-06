//! Provider management through `DrafterService`: create from a preset or a pasted entry,
//! update, delete, the owner and delegate views, model listing, and the connection test.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use postit_config::AiSettings;
use postit_core::{AccountId, AiGenerationId, SystemClock, SystemIdGenerator};
use postit_data::ai_generations::{AiGenerationRepo, GenerationOutcome, NewAiGeneration};
use postit_data::ai_providers::AiProviderRepo;
use postit_data::testkit::{callers, seed_user};
use postit_data::{Capability, OwnerScope};
use postit_drafter::testkit::{
    NoFrames, ScriptedFactory, ScriptedProvider, registry, text_descriptor,
};
use postit_drafter::{
    ChatResponse, CreateProvider, DrafterDeps, DrafterError, DrafterService, FailureReason, Finish,
    GenerateRequest, LlmError, Mode, ModelSource, Part, ProviderView, RemoteModel, UpdateProvider,
};
use postit_vault::Vault;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const KEY: &str = "sk-PRESET-SECRET-1";

fn vault() -> Arc<Vault> {
    Arc::new(
        Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap(),
    )
}

fn settings(allow_private: bool) -> AiSettings {
    AiSettings {
        request_timeout: Duration::from_secs(30),
        allow_private_endpoints: allow_private,
    }
}

struct Harness {
    svc: DrafterService,
    provider: Arc<ScriptedProvider>,
    factory: Arc<ScriptedFactory>,
}

fn harness_with(pool: &PgPool, allow_private: bool, provider: ScriptedProvider) -> Harness {
    let provider = Arc::new(provider);
    let factory = Arc::new(ScriptedFactory::new(provider.clone()));
    let svc = DrafterService::new(DrafterDeps {
        pool: pool.clone(),
        vault: vault(),
        clock: Arc::new(SystemClock),
        ids: Arc::new(SystemIdGenerator),
        registry: Arc::new(registry(vec![text_descriptor(
            "textish",
            500,
            postit_core::TextMeasure::Graphemes,
        )])),
        frames: Arc::new(NoFrames),
        factory: factory.clone(),
        settings: settings(allow_private),
    })
    .unwrap();
    Harness {
        svc,
        provider,
        factory,
    }
}

fn harness(pool: &PgPool) -> Harness {
    harness_with(pool, false, ScriptedProvider::new())
}

fn openai_preset(default_model: Option<&str>) -> CreateProvider {
    CreateProvider::Preset {
        key: "openai".to_owned(),
        api_key: Some(SecretString::from(KEY.to_owned())),
        default_model: default_model.map(str::to_owned),
    }
}

fn pasted() -> CreateProvider {
    CreateProvider::Pasted {
        entry: json!({
            "provider": {
                "mine": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "My gateway",
                    "options": {
                        "baseURL": "https://gateway.example.com/v1",
                        "apiKey": "sk-PASTED-SECRET",
                        "headers": {"X-Org": "org-HEADER-SECRET"}
                    },
                    "models": {
                        "fast": {"name": "Fast", "modalities": {"input": ["text", "image"]}},
                        "slow": {}
                    }
                }
            },
            "model": "mine/fast"
        }),
    }
}

fn owner_view(view: &ProviderView) -> OwnerFields {
    match view {
        ProviderView::Owner {
            id,
            preset,
            name,
            base_url,
            header_names,
            has_key,
            default_model,
            is_default,
            models,
            ..
        } => OwnerFields {
            id: *id,
            preset: preset.clone(),
            name: name.clone(),
            base_url: base_url.as_ref().map(ToString::to_string),
            header_names: header_names.clone(),
            has_key: *has_key,
            default_model: default_model.clone(),
            is_default: *is_default,
            models: models.iter().map(|m| m.id.clone()).collect(),
        },
        ProviderView::Delegate { .. } => panic!("expected an owner view: {view:?}"),
    }
}

struct OwnerFields {
    id: postit_core::AiProviderId,
    preset: String,
    name: String,
    base_url: Option<String>,
    header_names: Vec<String>,
    has_key: bool,
    default_model: Option<String>,
    is_default: bool,
    models: Vec<String>,
}

async fn audit_rows(pool: &PgPool, kind: &str) -> Vec<(Option<Uuid>, Option<Uuid>, Value)> {
    sqlx::query_as::<_, (Option<Uuid>, Option<Uuid>, Value)>(
        "SELECT actor_user_id, owner_id, details FROM audit_events WHERE kind = $1 ORDER BY at, id",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

fn has_violation_mentioning(err: &DrafterError, needle: &str) -> bool {
    match err {
        DrafterError::Validation(vs) => vs
            .iter()
            .any(|v| serde_json::to_string(v).unwrap().contains(needle)),
        _ => false,
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_from_preset_seals_key_and_audits(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);

    let view = h
        .svc
        .create_provider(&scope, openai_preset(Some("gpt-test")))
        .await
        .unwrap();
    let f = owner_view(&view);
    assert_eq!(f.preset, "openai");
    assert_eq!(f.name, "OpenAI");
    assert_eq!(f.base_url.as_deref(), Some("https://api.openai.com/v1"));
    assert!(f.has_key);
    assert!(f.header_names.is_empty());
    assert!(f.is_default, "the first provider becomes the default");
    assert_eq!(f.default_model.as_deref(), Some("gpt-test"));
    assert_eq!(f.models, ["gpt-test"]);

    // The key is sealed for this row, and nowhere in the clear.
    let (config, sealed): (Value, Vec<u8>) =
        sqlx::query_as("SELECT config, api_key FROM ai_providers WHERE id = $1")
            .bind(f.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!config.to_string().contains(KEY), "{config}");
    assert!(!String::from_utf8_lossy(&sealed).contains(KEY));
    let mut conn = pool.acquire().await.unwrap();
    let row = AiProviderRepo::get(&mut conn, &scope, f.id)
        .await
        .unwrap()
        .unwrap();
    let opened: Value = vault()
        .open(&row.secret.unwrap(), &AiProviderRepo::seal_context(f.id))
        .unwrap();
    assert_eq!(opened["api_key"], KEY);

    // One audit row with the provider ID, preset key, and name, and no key.
    let rows = audit_rows(&pool, "ai_provider_created").await;
    assert_eq!(rows.len(), 1);
    let (actor, owner_id, details) = &rows[0];
    assert_eq!(*actor, Some(owner.as_uuid()));
    assert_eq!(*owner_id, Some(owner.as_uuid()));
    assert_eq!(details["provider_id"], f.id.as_uuid().to_string());
    assert_eq!(details["preset"], "openai");
    assert_eq!(details["name"], "OpenAI");
    assert!(!details.to_string().contains(KEY));

    // The factory gets the opened key when the provider is used.
    h.provider.push_text("OK");
    h.svc.test_provider(&scope, f.id).await.unwrap();
    let build = h.factory.last_build().unwrap();
    assert_eq!(build.api_key.as_deref(), Some(KEY));
    assert_eq!(build.preset, Some("openai"));

    // A second provider is not the default; an unknown preset or a missing key is refused.
    let second = h
        .svc
        .create_provider(&scope, openai_preset(None))
        .await
        .unwrap();
    assert!(!owner_view(&second).is_default);
    let err = h
        .svc
        .create_provider(
            &scope,
            CreateProvider::Preset {
                key: "nope".to_owned(),
                api_key: Some(SecretString::from("k".to_owned())),
                default_model: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        has_violation_mentioning(&err, "ai.preset_unknown"),
        "{err:?}"
    );
    let err = h
        .svc
        .create_provider(
            &scope,
            CreateProvider::Preset {
                key: "openai".to_owned(),
                api_key: None,
                default_model: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        has_violation_mentioning(&err, "ai.api_key_required"),
        "{err:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_from_pasted_entry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);

    let view = h.svc.create_provider(&scope, pasted()).await.unwrap();
    let f = owner_view(&view);
    assert_eq!(f.preset, "custom");
    assert_eq!(f.name, "My gateway");
    assert_eq!(
        f.base_url.as_deref(),
        Some("https://gateway.example.com/v1")
    );
    assert_eq!(f.header_names, ["X-Org"]);
    assert!(f.has_key);
    assert_eq!(f.default_model.as_deref(), Some("fast"));
    assert_eq!(f.models, ["fast", "slow"]);
    let ser = serde_json::to_string(&view).unwrap();
    for secret in ["sk-PASTED-SECRET", "org-HEADER-SECRET"] {
        assert!(!ser.contains(secret), "{ser}");
        assert!(!format!("{view:?}").contains(secret));
    }
    let config: Value = sqlx::query_scalar("SELECT config FROM ai_providers WHERE id = $1")
        .bind(f.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!config.to_string().contains("SECRET"), "{config}");

    // The secret reaches the factory whole.
    h.provider.push_text("OK");
    h.svc.test_provider(&scope, f.id).await.unwrap();
    let build = h.factory.last_build().unwrap();
    assert_eq!(build.api_key.as_deref(), Some("sk-PASTED-SECRET"));
    assert_eq!(build.headers["X-Org"], "org-HEADER-SECRET");
    assert_eq!(build.preset, None);

    // A substitution is refused and nothing is stored.
    let before = count(&pool, "SELECT COUNT(*) FROM ai_providers").await;
    let err = h
        .svc
        .create_provider(
            &scope,
            CreateProvider::Pasted {
                entry: json!({
                    "npm": "@ai-sdk/openai",
                    "options": {"baseURL": "https://api.openai.com/v1", "apiKey": "{env:OPENAI_API_KEY}"}
                }),
            },
        )
        .await
        .unwrap_err();
    match &err {
        DrafterError::Validation(vs) => assert!(
            vs.iter()
                .any(|v| v.path.as_deref() == Some("options.apiKey")),
            "{vs:?}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM ai_providers").await,
        before
    );
    assert_eq!(audit_rows(&pool, "ai_provider_created").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn local_preset_refused_while_private_off(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let local = || CreateProvider::Preset {
        key: "ollama".to_owned(),
        api_key: None,
        default_model: Some("llama3".to_owned()),
    };

    let off = harness(&pool);
    let err = off.svc.create_provider(&scope, local()).await.unwrap_err();
    assert!(
        has_violation_mentioning(&err, "ai.allow_private_endpoints"),
        "{err:?}"
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ai_providers").await, 0);
    let presets = off.svc.presets();
    let find = |key: &str| presets.iter().find(|p| p.key == key).unwrap().clone();
    assert!(!find("ollama").available);
    assert!(find("ollama").local);
    assert!(find("openai").available);
    assert_eq!(presets.len(), 11);

    let on = harness_with(&pool, true, ScriptedProvider::new());
    assert!(on.svc.presets().iter().all(|p| p.available));
    let view = on.svc.create_provider(&scope, local()).await.unwrap();
    let f = owner_view(&view);
    assert_eq!(f.base_url.as_deref(), Some("http://localhost:11434/v1"));
    assert!(!f.has_key, "a local preset needs no key");
}

#[sqlx::test(migrations = "../data/migrations")]
#[allow(clippy::too_many_lines)]
async fn update_key_writes_key_changed_other_changes_updated(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    let id = owner_view(&h.svc.create_provider(&scope, pasted()).await.unwrap()).id;
    let updated = || audit_rows(&pool, "ai_provider_updated");
    let key_changed = || audit_rows(&pool, "ai_provider_key_changed");

    // A name change is an update, not a key change.
    let view = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                name: Some("Renamed".to_owned()),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(owner_view(&view).name, "Renamed");
    assert_eq!(updated().await.len(), 1);
    assert!(key_changed().await.is_empty());
    let details = &updated().await[0].2;
    assert_eq!(details["provider_id"], id.as_uuid().to_string());
    assert_eq!(details["name"], "Renamed");

    // A new key is a key change; the headers stay.
    h.svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                api_key: Some(SecretString::from("sk-NEW-SECRET".to_owned())),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated().await.len(), 1);
    assert_eq!(key_changed().await.len(), 1);
    assert!(!key_changed().await[0].2.to_string().contains("SECRET"));
    h.provider.push_text("OK");
    h.svc.test_provider(&scope, id).await.unwrap();
    let build = h.factory.last_build().unwrap();
    assert_eq!(build.api_key.as_deref(), Some("sk-NEW-SECRET"));
    assert_eq!(build.headers["X-Org"], "org-HEADER-SECRET");

    // New headers replace every header and the stored names; that is a key change too.
    let mut headers = BTreeMap::new();
    headers.insert(
        "X-Team".to_owned(),
        SecretString::from("team-SECRET".to_owned()),
    );
    let view = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                headers: Some(headers),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(owner_view(&view).header_names, ["X-Team"]);
    assert_eq!(key_changed().await.len(), 2);
    assert_eq!(updated().await.len(), 1);
    h.provider.push_text("OK");
    h.svc.test_provider(&scope, id).await.unwrap();
    let build = h.factory.last_build().unwrap();
    assert_eq!(build.api_key.as_deref(), Some("sk-NEW-SECRET"));
    assert_eq!(build.headers.len(), 1);
    assert_eq!(build.headers["X-Team"], "team-SECRET");

    // A bad header name is refused.
    let mut bad = BTreeMap::new();
    bad.insert("Bad Header".to_owned(), SecretString::from("v".to_owned()));
    let err = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                headers: Some(bad),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::Validation(_)), "{err:?}");

    // Base URL, default model, and models; the base URL is checked.
    let mut models = BTreeMap::new();
    models.insert("m1".to_owned(), postit_drafter::ModelConfig::default());
    let view = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                base_url: Some(Url::parse("https://other.example.com/v2/").unwrap()),
                models: Some(models),
                default_model: Some("m2".to_owned()),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    let f = owner_view(&view);
    assert_eq!(f.base_url.as_deref(), Some("https://other.example.com/v2/"));
    assert_eq!(f.default_model.as_deref(), Some("m2"));
    assert_eq!(f.models, ["m1", "m2"]);
    assert_eq!(updated().await.len(), 2);
    for url in [
        "http://other.example.com/v1",
        "https://127.0.0.1/v1",
        "https://user:pw@other.example.com/v1",
    ] {
        let err = h
            .svc
            .update_provider(
                &scope,
                id,
                UpdateProvider {
                    base_url: Some(Url::parse(url).unwrap()),
                    ..UpdateProvider::default()
                },
            )
            .await
            .unwrap_err();
        match &err {
            DrafterError::Validation(vs) => assert!(
                vs.iter().any(|v| v.path.as_deref() == Some("base_url")),
                "{url}: {vs:?}"
            ),
            other => panic!("{url}: {other:?}"),
        }
    }
    // A blank name is refused.
    let err = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                name: Some("  ".to_owned()),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::Validation(_)), "{err:?}");

    // An Anthropic base typed with /v1 is stored without it.
    let anth = h
        .svc
        .create_provider(
            &scope,
            CreateProvider::Preset {
                key: "anthropic".to_owned(),
                api_key: Some(SecretString::from("sk-ant".to_owned())),
                default_model: None,
            },
        )
        .await
        .unwrap();
    let anth_id = owner_view(&anth).id;
    let view = h
        .svc
        .update_provider(
            &scope,
            anth_id,
            UpdateProvider {
                base_url: Some(Url::parse("https://proxy.example.com/anthropic/v1").unwrap()),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        owner_view(&view).base_url.as_deref(),
        Some("https://proxy.example.com/anthropic")
    );

    // make_default moves the default.
    let view = h
        .svc
        .update_provider(
            &scope,
            anth_id,
            UpdateProvider {
                make_default: true,
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap();
    assert!(owner_view(&view).is_default);
    let list = h.svc.list_providers(&scope).await.unwrap();
    let defaults: Vec<_> = list
        .iter()
        .map(owner_view)
        .filter(|f| f.is_default)
        .map(|f| f.id)
        .collect();
    assert_eq!(defaults, [anth_id]);

    // An unknown provider is not found.
    let err = h
        .svc
        .update_provider(
            &scope,
            postit_core::AiProviderId::from_uuid(Uuid::now_v7()),
            UpdateProvider {
                name: Some("x".to_owned()),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_audits_and_keeps_generations(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    let id = owner_view(
        &h.svc
            .create_provider(&scope, openai_preset(Some("gpt-test")))
            .await
            .unwrap(),
    )
    .id;
    let generation = AiGenerationId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    AiGenerationRepo::insert(
        &mut conn,
        &scope,
        NewAiGeneration {
            id: generation,
            provider_id: Some(id),
            model: "gpt-test".to_owned(),
            mode: "post".to_owned(),
            prompt_hash: [7; 32],
            prompt: None,
            input_tokens: Some(10),
            output_tokens: Some(20),
            outcome: GenerationOutcome::Ok,
        },
    )
    .await
    .unwrap();
    drop(conn);

    h.svc.delete_provider(&scope, id).await.unwrap();
    let rows = audit_rows(&pool, "ai_provider_deleted").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2["provider_id"], id.as_uuid().to_string());
    assert_eq!(rows[0].2["preset"], "openai");
    assert_eq!(rows[0].2["name"], "OpenAI");
    assert!(h.svc.list_providers(&scope).await.unwrap().is_empty());

    let provider_id: Option<Uuid> =
        sqlx::query_scalar("SELECT provider_id FROM ai_generations WHERE id = $1")
            .bind(generation.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(provider_id, None, "the generation stays, with no provider");

    let err = h.svc.delete_provider(&scope, id).await.unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
    assert_eq!(audit_rows(&pool, "ai_provider_deleted").await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_sees_names_and_models_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let h = harness(&pool);
    h.svc.create_provider(&c.owner, pasted()).await.unwrap();

    for scope in [&c.edit, &c.publish, &c.limited] {
        let list = h.svc.list_providers(scope).await.unwrap();
        assert_eq!(list.len(), 1);
        let ProviderView::Delegate {
            name,
            models,
            default_model,
            is_default,
            preset,
            ..
        } = &list[0]
        else {
            panic!("expected a delegate view: {:?}", list[0]);
        };
        assert_eq!(name, "My gateway");
        assert_eq!(preset, "custom");
        assert_eq!(default_model.as_deref(), Some("fast"));
        assert!(*is_default);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["fast", "slow"]
        );
        let ser = serde_json::to_value(&list[0]).unwrap();
        let obj = ser.as_object().unwrap();
        for hidden in ["base_url", "header_names", "has_key", "protocol"] {
            assert!(!obj.contains_key(hidden), "{hidden} in {ser}");
        }
        for present in ["id", "name", "models", "default_model", "is_default"] {
            assert!(obj.contains_key(present), "{present} missing from {ser}");
        }
        let text = ser.to_string();
        assert!(
            !text.contains("gateway.example.com") && !text.contains("SECRET"),
            "{text}"
        );
    }

    let list = h.svc.list_providers(&c.owner).await.unwrap();
    assert!(matches!(list[0], ProviderView::Owner { .. }));
    let ser = serde_json::to_value(&list[0]).unwrap();
    assert!(
        ser.get("base_url").is_some() && ser.get("has_key").is_some(),
        "{ser}"
    );

    // Below edit, nothing; a stranger sees their own (empty) workspace.
    let err = h.svc.list_providers(&c.view).await.unwrap_err();
    assert!(
        matches!(err, DrafterError::Forbidden(Capability::Edit)),
        "{err:?}"
    );
    assert!(h.svc.list_providers(&c.stranger).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_provider_in_the_callers_view(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let h = harness(&pool);
    let created = h.svc.create_provider(&c.owner, pasted()).await.unwrap();
    let ProviderView::Owner { id, .. } = created else {
        panic!("expected an owner view: {created:?}");
    };

    let own = h.svc.get_provider(&c.owner, id).await.unwrap();
    assert_eq!(own, h.svc.list_providers(&c.owner).await.unwrap()[0]);
    for scope in [&c.edit, &c.publish, &c.limited] {
        let view = h.svc.get_provider(scope, id).await.unwrap();
        assert!(matches!(view, ProviderView::Delegate { .. }), "{view:?}");
    }
    let err = h.svc.get_provider(&c.view, id).await.unwrap_err();
    assert!(
        matches!(err, DrafterError::Forbidden(Capability::Edit)),
        "{err:?}"
    );
    let err = h.svc.get_provider(&c.stranger, id).await.unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
    h.svc.delete_provider(&c.owner, id).await.unwrap();
    let err = h.svc.get_provider(&c.owner, id).await.unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_cannot_manage(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let h = harness(&pool);
    let id = owner_view(&h.svc.create_provider(&c.owner, pasted()).await.unwrap()).id;

    for scope in [&c.edit, &c.publish] {
        let forbidden = |r: Result<_, DrafterError>| {
            matches!(r, Err(DrafterError::Forbidden(Capability::OwnerOnly)))
        };
        assert!(forbidden(
            h.svc.create_provider(scope, pasted()).await.map(|_| ())
        ));
        assert!(forbidden(
            h.svc
                .update_provider(
                    scope,
                    id,
                    UpdateProvider {
                        name: Some("x".to_owned()),
                        ..UpdateProvider::default()
                    }
                )
                .await
                .map(|_| ())
        ));
        assert!(forbidden(h.svc.delete_provider(scope, id).await));
        assert!(forbidden(h.svc.test_provider(scope, id).await.map(|_| ())));
    }
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ai_providers").await, 1);
    assert_eq!(h.provider.chat_calls(), 0);
    // A stranger cannot reach the owner's provider at all.
    let err = h.svc.delete_provider(&c.stranger, id).await.unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn models_merge_config_and_remote(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let h = harness_with(
        &pool,
        false,
        ScriptedProvider::new().models(Ok(vec![
            RemoteModel {
                id: "slow".to_owned(),
                name: Some("Slow (remote name)".to_owned()),
            },
            RemoteModel {
                id: "zeta".to_owned(),
                name: None,
            },
        ])),
    );
    let id = owner_view(&h.svc.create_provider(&c.owner, pasted()).await.unwrap()).id;

    // A delegate at edit may list models too.
    for scope in [&c.owner, &c.edit] {
        let list = h.svc.models(scope, id).await.unwrap();
        assert_eq!(list.remote_error, None);
        let got: Vec<_> = list
            .models
            .iter()
            .map(|m| (m.id.as_str(), m.name.as_deref(), m.source, m.images))
            .collect();
        assert_eq!(
            got,
            [
                ("fast", Some("Fast"), ModelSource::Config, true),
                ("slow", Some("Slow (remote name)"), ModelSource::Both, false),
                ("zeta", None, ModelSource::Remote, false),
            ]
        );
    }
    assert_eq!(h.provider.model_calls(), 2);
    let err = h.svc.models(&c.view, id).await.unwrap_err();
    assert!(
        matches!(err, DrafterError::Forbidden(Capability::Edit)),
        "{err:?}"
    );

    // A preset with no model listing never calls the provider.
    let minimax = owner_view(
        &h.svc
            .create_provider(
                &c.owner,
                CreateProvider::Preset {
                    key: "minimax".to_owned(),
                    api_key: Some(SecretString::from("mm".to_owned())),
                    default_model: Some("MiniMax-M2".to_owned()),
                },
            )
            .await
            .unwrap(),
    )
    .id;
    let list = h.svc.models(&c.owner, minimax).await.unwrap();
    assert_eq!(list.remote_error, None);
    assert_eq!(
        list.models
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["MiniMax-M2"]
    );
    assert_eq!(h.provider.model_calls(), 2);

    let err = h
        .svc
        .models(
            &c.owner,
            postit_core::AiProviderId::from_uuid(Uuid::now_v7()),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn models_remote_failure_keeps_config_models(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness_with(
        &pool,
        false,
        ScriptedProvider::new().models(Err(LlmError::new(FailureReason::Unavailable))),
    );
    let id = owner_view(&h.svc.create_provider(&scope, pasted()).await.unwrap()).id;
    let list = h.svc.models(&scope, id).await.unwrap();
    assert_eq!(list.remote_error, Some(FailureReason::Unavailable));
    assert_eq!(
        list.models
            .iter()
            .map(|m| (m.id.as_str(), m.source))
            .collect::<Vec<_>>(),
        [("fast", ModelSource::Config), ("slow", ModelSource::Config)]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn test_sends_tiny_chat_and_writes_no_generation(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness_with(
        &pool,
        false,
        ScriptedProvider::new()
            .answer(Ok(ChatResponse {
                text: String::new(),
                usage: None,
                finish: Finish::Length,
            }))
            .models(Ok(vec![
                RemoteModel {
                    id: "a".to_owned(),
                    name: None,
                },
                RemoteModel {
                    id: "b".to_owned(),
                    name: None,
                },
            ])),
    );
    let id = owner_view(&h.svc.create_provider(&scope, pasted()).await.unwrap()).id;

    let result = h.svc.test_provider(&scope, id).await.unwrap();
    assert!(
        result.ok,
        "a thinking model may spend all 5 tokens: {result:?}"
    );
    assert_eq!(result.reason, None);
    assert_eq!(result.models_found, Some(2));
    let requests = h.provider.requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.model, "fast");
    assert_eq!(req.max_tokens, 5);
    assert_eq!(req.temperature, None);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(
        req.messages[0].parts,
        [Part::Text("Reply with OK".to_owned())]
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ai_generations").await, 0);

    // A refused key fails the test with its reason.
    h.provider
        .push(Err(LlmError::new(FailureReason::KeyRejected)));
    let result = h.svc.test_provider(&scope, id).await.unwrap();
    assert!(!result.ok);
    assert_eq!(result.reason, Some(FailureReason::KeyRejected));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ai_generations").await, 0);
}

/// Review Focus 5: the owner deletes their default provider while a delegate has the composer
/// open. The delegate's next generate, with or without the stale `provider_id`, answers
/// `ProviderRequired`.
#[sqlx::test(migrations = "../data/migrations")]
async fn deleted_default_then_generate_requires_provider(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let h = harness(&pool);
    sqlx::query("INSERT INTO platforms (id, display_name) VALUES ('textish', 'Textish')")
        .execute(&pool)
        .await
        .unwrap();
    let account = AccountId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name)
         VALUES ($1, $2, 'textish', 'r-1', 'Shop')",
    )
    .bind(account.as_uuid())
    .bind(owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let id = owner_view(
        &h.svc
            .create_provider(&c.owner, openai_preset(Some("gpt-test")))
            .await
            .unwrap(),
    )
    .id;
    h.svc.delete_provider(&c.owner, id).await.unwrap();

    for provider_id in [None, Some(id)] {
        let err = h
            .svc
            .generate(
                &c.edit,
                GenerateRequest {
                    mode: Mode::Post,
                    prompt: "A new loaf".to_owned(),
                    targets: vec![account],
                    language: None,
                    media_ids: Vec::new(),
                    comment_id: None,
                    provider_id,
                    model: None,
                    link: None,
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, DrafterError::ProviderRequired),
            "{provider_id:?}: {err:?}"
        );
    }
    assert_eq!(h.provider.chat_calls(), 0);
}

/// A pasted entry's model IDs are checked like an update's: a blank or overlong ID is refused,
/// and nothing is stored.
#[sqlx::test(migrations = "../data/migrations")]
async fn pasted_model_ids_are_checked(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    for bad in [" ".to_owned(), "m".repeat(201)] {
        let err = h
            .svc
            .create_provider(
                &scope,
                CreateProvider::Pasted {
                    entry: json!({
                        "npm": "@ai-sdk/openai-compatible",
                        "options": {"baseURL": "https://gateway.example.com/v1"},
                        "models": {bad: {}}
                    }),
                },
            )
            .await
            .unwrap_err();
        assert!(
            has_violation_mentioning(&err, "ai.model_invalid"),
            "{err:?}"
        );
    }
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ai_providers").await, 0);
}

/// Every violation of an update comes back at once, the stored config's own included.
#[sqlx::test(migrations = "../data/migrations")]
async fn update_reports_every_violation(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    let id = owner_view(&h.svc.create_provider(&scope, pasted()).await.unwrap()).id;
    let models: BTreeMap<String, postit_drafter::ModelConfig> = serde_json::from_value(json!({
        "fast": {"name": "Has a \u{0} NUL"}
    }))
    .unwrap();
    let err = h
        .svc
        .update_provider(
            &scope,
            id,
            UpdateProvider {
                name: Some(String::new()),
                models: Some(models),
                ..UpdateProvider::default()
            },
        )
        .await
        .unwrap_err();
    let DrafterError::Validation(vs) = &err else {
        panic!("{err:?}");
    };
    let keys: Vec<String> = vs
        .iter()
        .map(|v| v.params["message"]["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(keys, ["ai.name_invalid", "ai.text_invalid"], "each once");
}

/// With no model to chat with and no model listing, the test asks for a default model instead
/// of blaming the provider.
#[sqlx::test(migrations = "../data/migrations")]
async fn test_without_model_or_listing_asks_for_a_model(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    let id = owner_view(
        &h.svc
            .create_provider(
                &scope,
                CreateProvider::Preset {
                    key: "minimax".to_owned(),
                    api_key: Some(SecretString::from(KEY.to_owned())),
                    default_model: None,
                },
            )
            .await
            .unwrap(),
    )
    .id;
    let err = h.svc.test_provider(&scope, id).await.unwrap_err();
    assert!(
        has_violation_mentioning(&err, "ai.model_required"),
        "{err:?}"
    );
    assert_eq!(h.provider.chat_calls(), 0);
    assert_eq!(h.provider.model_calls(), 0);
}

/// Two concurrent updates of one provider both land: the second reads after the first wrote.
#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_updates_keep_both_changes(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let h = harness(&pool);
    let id = owner_view(&h.svc.create_provider(&scope, pasted()).await.unwrap()).id;
    for round in 0..5 {
        let name = format!("Name {round}");
        let model = format!("m-{round}");
        let rename = h.svc.update_provider(
            &scope,
            id,
            UpdateProvider {
                name: Some(name.clone()),
                ..UpdateProvider::default()
            },
        );
        let remodel = h.svc.update_provider(
            &scope,
            id,
            UpdateProvider {
                default_model: Some(model.clone()),
                ..UpdateProvider::default()
            },
        );
        let (a, b) = tokio::join!(rename, remodel);
        a.unwrap();
        b.unwrap();
        let after = owner_view(&h.svc.list_providers(&scope).await.unwrap()[0]);
        assert_eq!(after.name, name, "round {round}: the rename was lost");
        assert_eq!(
            after.default_model.as_deref(),
            Some(model.as_str()),
            "round {round}: the model change was lost"
        );
    }
}

/// The real factory picks the protocol from the config, normalizes the base, and hands the
/// opened secret to the provider; a config with no base URL and no preset is refused.
#[tokio::test]
async fn http_factory_builds_each_protocol() {
    use postit_drafter::{HttpProviderFactory, ProviderConfig, ProviderFactory, ProviderSecret};
    use postit_http::{AddressPolicy, build_public_client};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let client = build_public_client(
        &postit_config::HttpSettings {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            user_agent: "postit-drafter-test/0".to_owned(),
            extra_ca_files: Vec::new(),
        },
        AddressPolicy {
            allow_private: true,
        },
    )
    .unwrap();
    let factory = HttpProviderFactory::new(client);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-oai"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "oai-model"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-ant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "ant-model", "display_name": "Ant"}],
            "has_more": false
        })))
        .mount(&server)
        .await;
    let secret = |key: &str| ProviderSecret {
        api_key: Some(SecretString::from(key.to_owned())),
        ..ProviderSecret::default()
    };
    let deadline = || tokio::time::Instant::now() + Duration::from_secs(10);

    let config: ProviderConfig = serde_json::from_value(json!({
        "protocol": "openai-compatible",
        "options": {"baseURL": format!("{}/v1", server.uri())}
    }))
    .unwrap();
    let models = factory
        .build(&config, secret("sk-oai"), None)
        .unwrap()
        .models(deadline())
        .await
        .unwrap();
    assert_eq!(models[0].id, "oai-model");

    // An Anthropic base typed with /v1 still reaches /v1/models once.
    let config: ProviderConfig = serde_json::from_value(json!({
        "protocol": "anthropic",
        "options": {"baseURL": format!("{}/v1", server.uri())}
    }))
    .unwrap();
    let models = factory
        .build(
            &config,
            secret("sk-ant"),
            postit_drafter::preset("anthropic"),
        )
        .unwrap()
        .models(deadline())
        .await
        .unwrap();
    assert_eq!(models[0].id, "ant-model");

    let Err(err) = factory.build(&ProviderConfig::default(), ProviderSecret::default(), None)
    else {
        panic!("a config with no protocol must be refused");
    };
    assert!(matches!(err, DrafterError::Validation(_)), "{err:?}");
    let config: ProviderConfig =
        serde_json::from_value(json!({"protocol": "openai-compatible"})).unwrap();
    let Err(err) = factory.build(&config, ProviderSecret::default(), None) else {
        panic!("a config with no base URL must be refused");
    };
    assert!(
        has_violation_mentioning(&err, "ai.base_url_required"),
        "{err:?}"
    );
}
