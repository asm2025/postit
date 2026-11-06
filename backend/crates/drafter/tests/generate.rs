//! Generation through `DrafterService`: every mode, the media context, validation against each
//! target's descriptor, the one repair, the generation row, and the access checks.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use emixdb::dto::Pagination;
use postit_config::AiSettings;
use postit_core::{
    AccountId, AccountRef, AiGenerationId, FrameExtractor, InboxItemId, MediaAssetId, MediaItem,
    PlatformId, PostId, PostKind, PostVersion, PublishSnapshot, SystemClock, SystemIdGenerator,
    TextMeasure, UserId, Variant, Violation, ViolationCode,
};
use postit_data::ai_generations::prompt_hash;
use postit_data::testkit::{callers, seed_user};
use postit_data::{Capability, OwnerScope};
use postit_drafter::testkit::{
    FakeFrames, NoFrames, SCRIPTED_USAGE, ScriptedFactory, ScriptedProvider, caption_max,
    composed_descriptor, email_descriptor, options_descriptor, registry, text_descriptor,
};
use postit_drafter::{
    ChatRequest, ChatResponse, CreateProvider, DrafterDeps, DrafterError, DrafterService,
    FailureReason, Finish, GenerateRequest, Generation, Mode, OutputMode, Part, Role, Usage,
};
use postit_vault::Vault;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const TEXT_MAX: u32 = 300;
const COMPOSED_MAX: u32 = 4_096;

fn vault() -> Arc<Vault> {
    Arc::new(
        Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap(),
    )
}

struct Harness {
    svc: DrafterService,
    provider: Arc<ScriptedProvider>,
    factory: Arc<ScriptedFactory>,
}

fn harness_with(pool: &PgPool, frames: Arc<dyn FrameExtractor>) -> Harness {
    let provider = Arc::new(ScriptedProvider::new());
    let factory = Arc::new(ScriptedFactory::new(provider.clone()));
    let svc = DrafterService::new(DrafterDeps {
        pool: pool.clone(),
        vault: vault(),
        clock: Arc::new(SystemClock),
        ids: Arc::new(SystemIdGenerator),
        registry: Arc::new(registry([
            text_descriptor("textish", TEXT_MAX, TextMeasure::Graphemes),
            options_descriptor("optionish", TEXT_MAX),
            composed_descriptor("telegramish", COMPOSED_MAX),
            email_descriptor(),
        ])),
        frames,
        factory: factory.clone(),
        settings: AiSettings {
            request_timeout: Duration::from_secs(30),
            allow_private_endpoints: false,
        },
    })
    .unwrap();
    Harness {
        svc,
        provider,
        factory,
    }
}

fn harness(pool: &PgPool) -> Harness {
    harness_with(pool, Arc::new(NoFrames))
}

/// An owner with the testkit platforms and an `openai` provider whose default is `gpt-test`.
async fn owner_with_provider(pool: &PgPool, h: &Harness) -> UserId {
    let owner = seed_user(pool, "active").await;
    seed_platforms(pool).await;
    h.svc
        .create_provider(&OwnerScope::own(owner), openai_preset())
        .await
        .unwrap();
    owner
}

fn openai_preset() -> CreateProvider {
    CreateProvider::Preset {
        key: "openai".to_owned(),
        api_key: Some(SecretString::from("sk-test-1".to_owned())),
        default_model: Some("gpt-test".to_owned()),
    }
}

fn pasted(models: &Value, hint: &str) -> CreateProvider {
    CreateProvider::Pasted {
        entry: json!({
            "provider": {
                "mine": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "My gateway",
                    "options": {"baseURL": "https://gateway.example.com/v1", "apiKey": "sk-gw"},
                    "models": models
                }
            },
            "model": format!("mine/{hint}")
        }),
    }
}

async fn seed_platforms(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES
             ('textish', 'Textish'), ('telegramish', 'Telegramish'), ('email', 'Email'),
             ('optionish', 'Optionish')
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_account(pool: &PgPool, owner: UserId, platform: &str) -> AccountId {
    let id = AccountId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name)
         VALUES ($1, $2, $3, $4, 'Shop')",
    )
    .bind(id.as_uuid())
    .bind(owner.as_uuid())
    .bind(platform)
    .bind(format!("r-{id}"))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_media(
    pool: &PgPool,
    owner: UserId,
    mime: &str,
    status: &str,
    duration_ms: Option<i64>,
) -> MediaAssetId {
    let id = MediaAssetId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO media_assets
             (id, owner_id, storage_key, mime, bytes, width, height, duration_ms, status)
         VALUES ($1, $2, $3, $4, 1000, 800, 600, $5, $6)",
    )
    .bind(id.as_uuid())
    .bind(owner.as_uuid())
    .bind(format!("k/{id}"))
    .bind(mime)
    .bind(duration_ms)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_post(pool: &PgPool, owner: UserId, text: &str) -> PostId {
    let id = PostId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO posts (id, owner_id, text) VALUES ($1, $2, $3)")
        .bind(id.as_uuid())
        .bind(owner.as_uuid())
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn set_store_prompts(pool: &PgPool, user: UserId, store: bool) {
    sqlx::query(
        "INSERT INTO user_preferences (user_id, store_ai_prompts) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET store_ai_prompts = $2",
    )
    .bind(user.as_uuid())
    .bind(store)
    .execute(pool)
    .await
    .unwrap();
}

fn request(targets: &[AccountId]) -> GenerateRequest {
    GenerateRequest {
        mode: Mode::Post,
        prompt: "Announce the spring menu".to_owned(),
        targets: targets.to_vec(),
        language: None,
        media_ids: Vec::new(),
        comment_id: None,
        provider_id: None,
        model: None,
        link: None,
    }
}

fn answer(variants: &[(AccountId, &str, &[&str])]) -> String {
    let items: Vec<Value> = variants
        .iter()
        .map(|(a, text, tags)| json!({"target": a.to_string(), "text": text, "tags": tags}))
        .collect();
    json!({ "variants": items }).to_string()
}

/// The repair's text: one user message still, with the repair appended as its last text part
/// (chat templates that need alternating roles refuse two user turns in a row).
fn repair_text(req: &ChatRequest) -> String {
    assert_eq!(
        req.messages.len(),
        1,
        "the repair stays in the one user message"
    );
    assert_eq!(req.messages[0].role, Role::User);
    req.messages[0]
        .parts
        .iter()
        .rev()
        .find_map(|p| match p {
            Part::Text(t) => Some(t.clone()),
            Part::Image { .. } => None,
        })
        .unwrap()
}

fn texts(req: &ChatRequest, message: usize) -> String {
    req.messages[message]
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            Part::Image { .. } => None,
        })
        .collect()
}

fn images(req: &ChatRequest) -> usize {
    req.messages[0]
        .parts
        .iter()
        .filter(|p| matches!(p, Part::Image { .. }))
        .count()
}

fn violation_keys(err: &DrafterError) -> Vec<String> {
    match err {
        DrafterError::Validation(vs) => vs.iter().map(key_of).collect(),
        other => panic!("expected a validation error: {other:?}"),
    }
}

fn key_of(v: &Violation) -> String {
    v.params
        .get("message")
        .and_then(|m| m["key"].as_str())
        .map_or_else(|| v.code.as_str().to_owned(), str::to_owned)
}

fn codes(g: &Generation, account: AccountId) -> Vec<String> {
    g.violations
        .get(&account)
        .map(|vs| vs.iter().map(key_of).collect())
        .unwrap_or_default()
}

struct Row {
    outcome: String,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    prompt: Option<String>,
    prompt_hash: Vec<u8>,
    created_by: Option<Uuid>,
    mode: String,
    model: String,
}

async fn rows(pool: &PgPool) -> Vec<Row> {
    sqlx::query_as::<
        _,
        (
            String,
            Option<i32>,
            Option<i32>,
            Option<String>,
            Vec<u8>,
            Option<Uuid>,
            String,
            String,
        ),
    >(
        "SELECT outcome, input_tokens, output_tokens, prompt, prompt_hash, created_by, mode, model
         FROM ai_generations ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| Row {
        outcome: r.0,
        input_tokens: r.1,
        output_tokens: r.2,
        prompt: r.3,
        prompt_hash: r.4,
        created_by: r.5,
        mode: r.6,
        model: r.7,
    })
    .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn valid_first_answer_one_call(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider
        .push_text(answer(&[(a, "Spring menu is here", &["#spring", "food"])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 1);
    assert!(g.violations.is_empty(), "{:?}", g.violations);
    assert!(g.notes.is_empty());
    assert_eq!(g.usage, Some(SCRIPTED_USAGE));
    assert_eq!(g.model, "gpt-test");
    assert!(g.draft.post.disclose_ai_generated);
    assert_eq!(g.draft.post.kind, PostKind::Post);
    assert_eq!(g.draft.post.text, "Spring menu is here");
    assert_eq!(g.draft.overrides.len(), 1);
    assert_eq!(
        g.draft.overrides[&a].tags,
        Some(vec!["spring".to_owned(), "food".to_owned()])
    );

    let req = h.provider.last_request().unwrap();
    assert_eq!(req.temperature, None);
    assert_eq!(req.max_tokens, 8_192);
    assert!(matches!(req.output, OutputMode::JsonSchema { .. }));
    assert!(texts(&req, 0).contains("Announce the spring menu"));
    assert!(texts(&req, 0).contains(&a.to_string()));
    assert!(req.system.contains("Never invent links"));

    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "ok");
    assert_eq!(rows[0].mode, "post");
    assert_eq!(rows[0].model, "gpt-test");
    assert_eq!(rows[0].input_tokens, Some(100));
    assert_eq!(rows[0].output_tokens, Some(20));
    assert_eq!(rows[0].created_by, Some(owner.as_uuid()));
    let id: Uuid = sqlx::query_scalar("SELECT id FROM ai_generations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(g.id, AiGenerationId::from_uuid(id));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn over_limit_text_repaired(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let long = "x".repeat(400);
    h.provider.push_text(answer(&[(a, &long, &[])]));
    h.provider.push_text(answer(&[(a, "Short and sweet", &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    assert!(g.violations.is_empty(), "{:?}", g.violations);
    assert_eq!(g.draft.post.text, "Short and sweet");
    assert_eq!(
        g.usage,
        Some(Usage {
            input_tokens: 200,
            output_tokens: 40
        })
    );
    let repair = h.provider.last_request().unwrap();
    let last = repair_text(&repair);
    assert!(last.contains("text is 400 characters"), "{last}");
    // The original request is still the message's first part.
    assert!(texts(&repair, 0).starts_with(&texts(&h.provider.requests()[0], 0)));
    assert!(last.contains("the limit is 300"), "{last}");
    // Both calls share one deadline.
    let reqs = h.provider.requests();
    assert_eq!(reqs[0].deadline, reqs[1].deadline);
    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "ok");
    assert_eq!(rows[0].input_tokens, Some(200));
    assert_eq!(rows[0].output_tokens, Some(40));
}

/// Review Focus 6: the text fits alone but not once the link and the tags are composed in.
#[sqlx::test(migrations = "../data/migrations")]
async fn composed_target_counts_link_and_tags(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "telegramish").await;
    let text = "a".repeat(4_060);
    h.provider
        .push_text(answer(&[(a, &text, &["bread", "cake", "fresh"])]));
    h.provider
        .push_text(answer(&[(a, "Fresh bread", &["bread", "cake", "fresh"])]));
    let mut req = request(&[a]);
    req.link = Some(Url::parse("https://bakery.example/spring").unwrap());

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    assert_eq!(
        h.provider.chat_calls(),
        2,
        "the composed text is over 4,096"
    );
    assert!(g.violations.is_empty(), "{:?}", g.violations);
    let first = h.provider.requests()[0].clone();
    assert!(texts(&first, 0).contains("toward the limit of 4096"));
    let repair = h.provider.last_request().unwrap();
    let last = repair_text(&repair);
    let problems = &last[last.find("Problems:").unwrap()..];
    assert_eq!(
        problems.matches(&a.to_string()).count(),
        1,
        "one text_too_long problem: {problems}"
    );
    assert!(problems.contains("the limit is 4096"), "{problems}");
    assert!(problems.contains("link"), "{problems}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn photo_target_briefed_with_caption_limit(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "telegramish").await;
    let photo = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    h.provider.push_text(answer(&[(a, "Our new cake", &[])]));
    let mut req = request(&[a]);
    req.media_ids = vec![photo];

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    let text = texts(&h.provider.requests()[0], 0);
    let caption = caption_max(COMPOSED_MAX);
    assert!(text.contains(&format!("at most {caption} ")), "{text}");
    assert!(!text.contains("at most 4096"), "{text}");
    assert_eq!(
        g.draft.post.media,
        vec![MediaItem {
            asset: photo,
            alt_text: None
        }]
    );
    assert!(g.violations.is_empty(), "{:?}", g.violations);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_invalid_returned_with_violations(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let long = "y".repeat(350);
    h.provider.push_text(answer(&[(a, &long, &[])]));
    h.provider.push_text(answer(&[(a, &long, &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    assert_eq!(codes(&g, a), ["text_too_long"]);
    assert_eq!(g.draft.overrides[&a].text.as_deref(), Some(long.as_str()));
    assert_eq!(rows(&pool).await[0].outcome, "ok");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unparseable_twice_is_invalid_output(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push_text("I cannot help with that.");
    h.provider.push_text("Still not JSON.");

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::ProviderFailed(e) if e.reason == FailureReason::InvalidOutput),
        "{err:?}"
    );
    assert_eq!(h.provider.chat_calls(), 2);
    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "invalid_output");
    assert_eq!(rows[0].input_tokens, Some(200));
}

/// Review Focus 3: the model stops at its token limit mid-JSON.
#[sqlx::test(migrations = "../data/migrations")]
async fn length_finish_triggers_repair(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push(Ok(ChatResponse {
        text: format!("{{\"variants\": [{{\"target\": \"{a}\", \"text\": \"Spr"),
        usage: Some(SCRIPTED_USAGE),
        finish: Finish::Length,
    }));
    h.provider.push_text(answer(&[(a, "Spring is here", &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    assert_eq!(g.draft.post.text, "Spring is here");
    let repair = h.provider.last_request().unwrap();
    assert!(repair_text(&repair).contains("cut off"));
}

/// Review Focus 4: the model answers for one of two targets.
#[sqlx::test(migrations = "../data/migrations")]
async fn missing_target_triggers_repair_and_is_reported(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let b = seed_account(&pool, owner, "textish").await;
    h.provider.push_text(answer(&[(a, "For a", &[])]));
    h.provider.push_text(answer(&[(a, "For a again", &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a, b]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    let repair = h.provider.last_request().unwrap();
    let last = repair_text(&repair);
    assert!(
        last.contains(&format!("{b}: no variant was returned")),
        "{last}"
    );
    assert!(codes(&g, b).contains(&"ai.target_missing".to_owned()));
    assert!(codes(&g, a).is_empty());
    assert_eq!(
        g.draft.overrides.len(),
        2,
        "every requested target comes back"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn media_violation_does_not_repair(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let photo = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    h.provider.push_text(answer(&[(a, "Look at this", &[])]));
    let mut req = request(&[a]);
    req.media_ids = vec![photo];

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    assert_eq!(
        h.provider.chat_calls(),
        1,
        "a media problem is not the model's"
    );
    assert!(
        g.violations[&a]
            .iter()
            .any(|v| v.code == ViolationCode::FormatUnsupported),
        "{:?}",
        g.violations
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn provider_failure_before_answer_writes_no_row(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push(Err(FailureReason::Unavailable.into()));

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::ProviderFailed(e) if e.reason == FailureReason::Unavailable),
        "{err:?}"
    );
    assert!(rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn provider_failure_after_repair_answer_writes_failed_row(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push_text(answer(&[(a, &"z".repeat(500), &[])]));
    h.provider.push(Err(FailureReason::RateLimited.into()));

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::ProviderFailed(e) if e.reason == FailureReason::RateLimited),
        "{err:?}"
    );
    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "failed");
    assert_eq!(
        rows[0].input_tokens,
        Some(100),
        "the first answer was paid for"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_provider_is_provider_required(pool: PgPool) {
    let h = harness(&pool);
    let owner = seed_user(&pool, "active").await;
    seed_platforms(&pool).await;
    let a = seed_account(&pool, owner, "textish").await;

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::ProviderRequired), "{err:?}");
    assert_eq!(h.provider.chat_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn explicit_provider_and_model_override_default(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let scope = OwnerScope::own(owner);
    let a = seed_account(&pool, owner, "textish").await;
    let gateway = h
        .svc
        .create_provider(&scope, pasted(&json!({"fast": {}, "slow": {}}), "fast"))
        .await
        .unwrap();
    let gateway_id = match gateway {
        postit_drafter::ProviderView::Owner { id, is_default, .. } => {
            assert!(!is_default);
            id
        }
        other @ postit_drafter::ProviderView::Delegate { .. } => panic!("{other:?}"),
    };
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    let mut req = request(&[a]);
    req.provider_id = Some(gateway_id);
    req.model = Some("slow".to_owned());

    let g = h.svc.generate(&scope, req).await.unwrap();
    assert_eq!(g.provider_id, gateway_id);
    assert_eq!(g.model, "slow");
    assert_eq!(h.provider.last_request().unwrap().model, "slow");
    assert_eq!(
        h.factory.last_build().unwrap().config.name.as_deref(),
        Some("My gateway")
    );
    assert_eq!(rows(&pool).await[0].model, "slow");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn max_tokens_from_model_limit(pool: PgPool) {
    let h = harness(&pool);
    let owner = seed_user(&pool, "active").await;
    seed_platforms(&pool).await;
    let scope = OwnerScope::own(owner);
    h.svc
        .create_provider(
            &scope,
            pasted(&json!({"small": {"limit": {"output": 1000}}}), "small"),
        )
        .await
        .unwrap();
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push_text(answer(&[(a, "Hi", &[])]));

    h.svc.generate(&scope, request(&[a])).await.unwrap();
    let req = h.provider.last_request().unwrap();
    assert_eq!(req.model, "small");
    assert_eq!(req.max_tokens, 1_000);
    assert!(
        matches!(req.output, OutputMode::JsonObject),
        "a custom provider defaults to json_object"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn images_sent_to_image_model(pool: PgPool) {
    let frames = Arc::new(FakeFrames::new());
    let h = harness_with(&pool, frames.clone());
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let one = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    let two = seed_media(&pool, owner, "image/png", "ready", None).await;
    h.provider.push_text(answer(&[(a, "Look", &[])]));
    let mut req = request(&[a]);
    req.media_ids = vec![one, two];

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    let sent = h.provider.last_request().unwrap();
    assert_eq!(images(&sent), 2);
    assert_eq!(frames.stills(), 2);
    assert!(texts(&sent, 0).contains("<media_note>"));
    assert!(
        !g.notes.iter().any(|n| n.key == "ai.media_ignored"),
        "{:?}",
        g.notes
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn video_sends_frames_and_duration(pool: PgPool) {
    let frames = Arc::new(FakeFrames::new());
    let h = harness_with(&pool, frames.clone());
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let video = seed_media(&pool, owner, "video/mp4", "ready", Some(12_000)).await;
    h.provider.push_text(answer(&[(a, "Watch", &[])]));
    let mut req = request(&[a]);
    req.media_ids = vec![video];

    h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    let sent = h.provider.last_request().unwrap();
    assert_eq!(images(&sent), 4, "four frames per video");
    assert_eq!(frames.videos(), 1);
    let text = texts(&sent, 0);
    assert!(text.contains("12 seconds"), "{text}");
    assert!(text.contains("4 frames"), "{text}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn text_only_model_gets_no_images_and_note(pool: PgPool) {
    let frames = Arc::new(FakeFrames::new());
    let h = harness_with(&pool, frames.clone());
    let owner = seed_user(&pool, "active").await;
    seed_platforms(&pool).await;
    let scope = OwnerScope::own(owner);
    h.svc
        .create_provider(
            &scope,
            pasted(
                &json!({"plain": {"modalities": {"input": ["text"]}}}),
                "plain",
            ),
        )
        .await
        .unwrap();
    let a = seed_account(&pool, owner, "textish").await;
    let photo = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    let mut req = request(&[a]);
    req.media_ids = vec![photo];

    let g = h.svc.generate(&scope, req).await.unwrap();
    let sent = h.provider.last_request().unwrap();
    assert_eq!(images(&sent), 0);
    assert_eq!(frames.stills(), 0);
    assert!(
        g.notes.iter().any(|n| n.key == "ai.media_ignored"),
        "{:?}",
        g.notes
    );
    assert!(texts(&sent, 0).contains("cannot see"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn more_than_four_images_noted(pool: PgPool) {
    let frames = Arc::new(FakeFrames::new());
    let h = harness_with(&pool, frames.clone());
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(seed_media(&pool, owner, "image/jpeg", "ready", None).await);
    }
    h.provider.push_text(answer(&[(a, "Look", &[])]));
    let mut req = request(&[a]);
    req.media_ids = ids;

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    assert_eq!(images(&h.provider.last_request().unwrap()), 4);
    assert_eq!(frames.stills(), 4);
    assert!(
        g.notes.iter().any(|n| n.key == "ai.media_truncated"),
        "{:?}",
        g.notes
    );
}

/// Stills and frames together stop at eight pictures: two videos' frames fill them, so the third
/// video is not cut and the image after it is left out, both noted.
#[sqlx::test(migrations = "../data/migrations")]
async fn pictures_capped_across_videos_and_images(pool: PgPool) {
    let frames = Arc::new(FakeFrames::new());
    let h = harness_with(&pool, frames.clone());
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(seed_media(&pool, owner, "video/mp4", "ready", Some(5_000)).await);
    }
    ids.push(seed_media(&pool, owner, "image/jpeg", "ready", None).await);
    h.provider.push_text(answer(&[(a, "Watch", &[])]));
    let mut req = request(&[a]);
    req.media_ids = ids;

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    let sent = h.provider.last_request().unwrap();
    assert_eq!(images(&sent), 8);
    assert_eq!(frames.videos(), 2, "the third video is not cut");
    assert_eq!(frames.stills(), 0, "no room is left for the image");
    let text = texts(&sent, 0);
    assert!(text.contains("it is not shown"), "{text}");
    assert!(
        text.contains("1 image more on the post is not shown"),
        "{text}"
    );
    assert!(
        g.notes.iter().any(|n| n.key == "ai.media_truncated"),
        "{:?}",
        g.notes
    );
}

/// The roadmap's random mode is a prompt plus optional media: media alone is not a request.
#[sqlx::test(migrations = "../data/migrations")]
async fn blank_prompt_refused_even_with_media(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let photo = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    let mut req = request(&[a]);
    req.prompt = "  ".to_owned();
    req.media_ids = vec![photo];
    let err = h
        .svc
        .generate(&OwnerScope::own(owner), req)
        .await
        .unwrap_err();
    assert_eq!(violation_keys(&err), ["ai.prompt_required"]);
    assert_eq!(h.provider.chat_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_media_is_not_found(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let other = seed_user(&pool, "active").await;
    let theirs = seed_media(&pool, other, "image/jpeg", "ready", None).await;
    for id in [MediaAssetId::from_uuid(Uuid::now_v7()), theirs] {
        let mut req = request(&[a]);
        req.media_ids = vec![id];
        let err = h
            .svc
            .generate(&OwnerScope::own(owner), req)
            .await
            .unwrap_err();
        assert!(matches!(err, DrafterError::NotFound), "{err:?}");
    }
    assert_eq!(h.provider.chat_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unready_media_is_validation(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let ready = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    let busy = seed_media(&pool, owner, "image/jpeg", "processing", None).await;
    let mut req = request(&[a]);
    req.media_ids = vec![ready, busy];

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), req)
        .await
        .unwrap_err();
    let DrafterError::Validation(vs) = &err else {
        panic!("{err:?}");
    };
    assert_eq!(vs.len(), 1);
    assert_eq!(key_of(&vs[0]), "ai.media_not_ready");
    assert_eq!(vs[0].path.as_deref(), Some("media_ids[1]"));
    assert_eq!(h.provider.chat_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn prompt_stored_only_with_store_ai_prompts(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let c = callers(&pool, owner, &[]).await;
    let key = vault().derive_key("ai_prompt_hash").unwrap();
    let expected = prompt_hash(key.as_ref(), owner, "Announce the spring menu").to_vec();

    // Nobody keeps prompts.
    h.provider.push_text(answer(&[(a, "One", &[])]));
    h.svc.generate(&c.owner, request(&[a])).await.unwrap();
    // The delegate keeps prompts, the owner does not: the owner's choice holds.
    set_store_prompts(&pool, c.edit.actor, true).await;
    h.provider.push_text(answer(&[(a, "Two", &[])]));
    h.svc.generate(&c.edit, request(&[a])).await.unwrap();
    // The owner keeps prompts: a delegate's prompt is kept too.
    set_store_prompts(&pool, owner, true).await;
    h.provider.push_text(answer(&[(a, "Three", &[])]));
    h.svc.generate(&c.edit, request(&[a])).await.unwrap();

    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].prompt, None);
    assert_eq!(rows[1].prompt, None);
    assert_eq!(rows[2].prompt.as_deref(), Some("Announce the spring menu"));
    for row in &rows {
        assert_eq!(row.prompt_hash, expected);
    }
    assert_eq!(rows[1].created_by, Some(c.edit.actor.as_uuid()));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limits_enforced(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let scope = OwnerScope::own(owner);
    let random = || AccountId::from_uuid(Uuid::now_v7());

    let mut long = request(&[a]);
    long.prompt = "é".repeat(8_001);
    let mut none = request(&[]);
    none.targets.clear();
    let many = request(&(0..21).map(|_| random()).collect::<Vec<_>>());
    let mut media = request(&[a]);
    media.media_ids = (0..11)
        .map(|_| MediaAssetId::from_uuid(Uuid::now_v7()))
        .collect();
    for (req, key) in [
        (long, "ai.prompt_too_long"),
        (none, "ai.targets_count"),
        (many, "ai.targets_count"),
        (media, "ai.media_too_many"),
    ] {
        let err = h.svc.generate(&scope, req).await.unwrap_err();
        assert!(violation_keys(&err).contains(&key.to_owned()), "{err:?}");
    }
    // Exactly the limit is fine.
    let mut at_limit = request(&[a]);
    at_limit.prompt = "é".repeat(8_000);
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    h.svc.generate(&scope, at_limit).await.unwrap();
    assert_eq!(h.provider.chat_calls(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_refused_target_outside_limit(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let inside = seed_account(&pool, owner, "textish").await;
    let outside = seed_account(&pool, owner, "textish").await;
    let c = callers(&pool, owner, &[inside]).await;

    for targets in [vec![outside], vec![inside, outside]] {
        let err = h
            .svc
            .generate(&c.limited, request(&targets))
            .await
            .unwrap_err();
        assert!(matches!(err, DrafterError::NotFound), "{err:?}");
    }
    assert_eq!(h.provider.chat_calls(), 0);
    h.provider.push_text(answer(&[(inside, "Hi", &[])]));
    h.svc
        .generate(&c.limited, request(&[inside]))
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn view_delegate_cannot_generate(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let c = callers(&pool, owner, &[]).await;
    let err = h.svc.generate(&c.view, request(&[a])).await.unwrap_err();
    assert!(
        matches!(err, DrafterError::Forbidden(Capability::Edit)),
        "{err:?}"
    );
    assert_eq!(h.provider.chat_calls(), 0);
}

/// A published post on `account`, a later edit of its text, and a comment on it.
async fn seed_comment(
    pool: &PgPool,
    owner: UserId,
    account: AccountId,
    remote_post_id: Option<&str>,
) -> InboxItemId {
    let post = seed_post(pool, owner, "Edited later").await;
    let snapshot = PublishSnapshot {
        post_id: post,
        post_version: PostVersion(1),
        variants: vec![Variant {
            target: AccountRef {
                platform: PlatformId::new("textish").unwrap(),
                account,
                remote_id: "r".to_owned(),
            },
            kind: PostKind::Post,
            text: "Published sourdough text".to_owned(),
            media: Vec::new(),
            link: None,
            tags: Vec::new(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options: serde_json::Map::new(),
        }],
    };
    let job = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, status)
         VALUES ($1, $2, $3, 1, $4, now(), 'published')",
    )
    .bind(job)
    .bind(owner.as_uuid())
    .bind(post.as_uuid())
    .bind(serde_json::to_value(&snapshot).unwrap())
    .execute(pool)
    .await
    .unwrap();
    let delivery = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO deliveries (id, owner_id, job_id, post_id, account_id, operation, state, idempotency_key, remote_id)
         VALUES ($1, $2, $3, $4, $5, 'publish', 'published', $6, 'p-1')",
    )
    .bind(delivery)
    .bind(owner.as_uuid())
    .bind(job)
    .bind(post.as_uuid())
    .bind(account.as_uuid())
    .bind(delivery.as_bytes().to_vec())
    .execute(pool)
    .await
    .unwrap();
    let item = InboxItemId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO inbox_items (id, owner_id, account_id, remote_comment_id, remote_post_id, delivery_id, author_name, text, received_at)
         VALUES ($1, $2, $3, 'c-1', $4, $5, 'Mallory', 'Love the bread!', now())",
    )
    .bind(item.as_uuid())
    .bind(owner.as_uuid())
    .bind(account.as_uuid())
    .bind(remote_post_id)
    .bind(delivery)
    .execute(pool)
    .await
    .unwrap();
    item
}

fn reply_request(targets: &[AccountId], item: InboxItemId) -> GenerateRequest {
    GenerateRequest {
        mode: Mode::Reply,
        prompt: String::new(),
        comment_id: Some(item),
        ..request(targets)
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reply_mode_uses_item_and_one_target(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let scope = OwnerScope::own(owner);
    let a = seed_account(&pool, owner, "textish").await;
    let b = seed_account(&pool, owner, "textish").await;
    let item = seed_comment(&pool, owner, a, Some("p-1")).await;

    let err = h
        .svc
        .generate(&scope, reply_request(&[a, b], item))
        .await
        .unwrap_err();
    assert!(violation_keys(&err).contains(&"ai.reply_one_target".to_owned()));
    let err = h
        .svc
        .generate(&scope, reply_request(&[b], item))
        .await
        .unwrap_err();
    assert!(violation_keys(&err).contains(&"ai.reply_account_mismatch".to_owned()));
    assert_eq!(h.provider.chat_calls(), 0);

    h.provider
        .push_text(answer(&[(a, "Thank you, Mallory!", &["nope"])]));
    let g = h
        .svc
        .generate(&scope, reply_request(&[a], item))
        .await
        .unwrap();
    assert!(g.violations.is_empty(), "{:?}", g.violations);
    assert_eq!(g.draft.post.kind, PostKind::Comment);
    let reply_to = g.draft.post.reply_to.clone().unwrap();
    assert_eq!(reply_to.remote_post_id, "p-1");
    assert_eq!(reply_to.remote_comment_id.as_deref(), Some("c-1"));
    assert!(g.draft.post.tags.is_empty(), "a reply carries no tags");
    let text = texts(&h.provider.last_request().unwrap(), 0);
    assert!(text.contains("Published sourdough text"), "{text}");
    assert!(!text.contains("Edited later"), "{text}");
    assert!(text.contains("Love the bread!"), "{text}");
    assert!(text.contains("Mallory"), "{text}");
    assert_eq!(rows(&pool).await[0].mode, "reply");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reply_without_remote_post_id_is_validation(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let item = seed_comment(&pool, owner, a, None).await;

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), reply_request(&[a], item))
        .await
        .unwrap_err();
    let DrafterError::Validation(vs) = &err else {
        panic!("{err:?}");
    };
    assert_eq!(key_of(&vs[0]), "ai.reply_target_unknown");
    assert_eq!(vs[0].path.as_deref(), Some("comment_id"));
    assert_eq!(h.provider.chat_calls(), 0);

    let err = h
        .svc
        .generate(
            &OwnerScope::own(owner),
            reply_request(&[a], InboxItemId::from_uuid(Uuid::now_v7())),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rewrite_uses_text_as_source(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider
        .push_text(answer(&[(a, "Bread, fresh daily", &[])]));

    let g = h
        .svc
        .rewrite(&OwnerScope::own(owner), "Fresh bread every day", &[a])
        .await
        .unwrap();
    assert_eq!(g.draft.post.text, "Bread, fresh daily");
    let req = h.provider.last_request().unwrap();
    assert!(
        texts(&req, 0).contains("<text_to_rewrite>\nFresh bread every day\n</text_to_rewrite>")
    );
    assert!(req.system.contains("rewrite"));
    assert_eq!(rows(&pool).await[0].mode, "rewrite");
}

/// The text to rewrite is the prompt, under the same 8,000-character limit, and says so.
#[sqlx::test(migrations = "../data/migrations")]
async fn rewrite_text_over_the_limit_refused(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let err = h
        .svc
        .rewrite(&OwnerScope::own(owner), &"é".repeat(8_001), &[a])
        .await
        .unwrap_err();
    let DrafterError::Validation(found) = &err else {
        panic!("expected a validation error: {err:?}");
    };
    assert_eq!(key_of(&found[0]), "ai.prompt_too_long");
    assert!(
        found[0].params["message"]["en"]
            .as_str()
            .unwrap()
            .contains("text to rewrite"),
        "{found:?}"
    );
    assert_eq!(h.provider.chat_calls(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn link_generation_sets_post_id(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let scope = OwnerScope::own(owner);
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    let g = h.svc.generate(&scope, request(&[a])).await.unwrap();
    let post = seed_post(&pool, owner, "Hi").await;

    assert!(h.svc.link_generation(&scope, g.id, post).await.unwrap());
    let linked: Option<Uuid> = sqlx::query_scalar("SELECT post_id FROM ai_generations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(linked, Some(post.as_uuid()));
    let unknown = AiGenerationId::from_uuid(Uuid::now_v7());
    assert!(!h.svc.link_generation(&scope, unknown, post).await.unwrap());
    // Another owner's post is not found.
    let other = seed_user(&pool, "active").await;
    let theirs = seed_post(&pool, other, "x").await;
    let err = h
        .svc
        .link_generation(&scope, g.id, theirs)
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn generations_listed_owner_only(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let c = callers(&pool, owner, &[]).await;
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    let g = h.svc.generate(&c.edit, request(&[a])).await.unwrap();

    let page = h
        .svc
        .generations(&c.owner, Pagination::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.data[0].id, g.id);
    assert_eq!(page.data[0].actor, Some(c.edit.actor));
    let err = h
        .svc
        .generations(&c.edit, Pagination::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::Forbidden(Capability::OwnerOnly)),
        "{err:?}"
    );
    let stranger = h
        .svc
        .generations(&c.stranger, Pagination::default())
        .await
        .unwrap();
    assert_eq!(stranger.total, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn email_target_gets_a_document(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "email").await;
    // No subject: an email violation, which the repair fixes.
    let without = json!({"variants": [{"target": a.to_string(), "text": "Summary", "tags": [],
        "email": {"subject": "", "preheader": "", "paragraphs": ["One"]}}]});
    let with = json!({"variants": [{"target": a.to_string(), "text": "Summary", "tags": [],
        "email": {"subject": "New cakes", "preheader": "Fresh", "paragraphs": ["One", "Two"]}}]});
    h.provider.push_text(without.to_string());
    h.provider.push_text(with.to_string());
    let mut req = request(&[a]);
    req.language = Some("ar".to_owned());

    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    assert!(g.violations.is_empty(), "{:?}", g.violations);
    let doc = g.draft.overrides[&a].email.as_ref().unwrap();
    assert_eq!(doc.subject, "New cakes");
    assert_eq!(doc.language, postit_core::EmailLanguage::Ar);
    assert_eq!(doc.blocks.len(), 2);
    let first = h.provider.requests()[0].clone();
    assert!(first.system.contains("\"subject\""));
    assert!(texts(&first, 0).contains("Language: ar"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn generation_debug_hides_content(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider
        .push_text(answer(&[(a, "SECRET-DRAFT-TEXT", &["SECRETTAG"])]));
    let mut req = request(&[a]);
    req.prompt = "SECRET-USER-PROMPT".to_owned();
    let shown = format!("{req:?}");
    assert!(!shown.contains("SECRET"), "{shown}");
    let g = h.svc.generate(&OwnerScope::own(owner), req).await.unwrap();
    let shown = format!("{g:?}");
    assert!(!shown.contains("SECRET"), "{shown}");
    assert!(shown.contains("gpt-test"), "{shown}");
}

/// The provider is deleted while it answers: the draft still comes back, and the paid call is
/// recorded with no provider.
#[sqlx::test(migrations = "../data/migrations")]
async fn provider_deleted_during_the_call_keeps_the_row(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let scope = OwnerScope::own(owner);
    let a = seed_account(&pool, owner, "textish").await;
    let id = match &h.svc.list_providers(&scope).await.unwrap()[0] {
        postit_drafter::ProviderView::Owner { id, .. } => *id,
        other @ postit_drafter::ProviderView::Delegate { .. } => panic!("{other:?}"),
    };
    h.provider.push_after(
        Duration::from_millis(800),
        Ok(ChatResponse {
            text: answer(&[(a, "Hi", &[])]),
            usage: Some(SCRIPTED_USAGE),
            finish: Finish::Stop,
        }),
    );
    let delete = async {
        // Wait until the call is out, then delete the provider under it.
        while h.provider.chat_calls() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        h.svc.delete_provider(&scope, id).await.unwrap();
    };
    let (generated, ()) = tokio::join!(h.svc.generate(&scope, request(&[a])), delete);
    let g = generated.unwrap();
    assert_eq!(g.draft.post.text, "Hi");
    let provider: Option<Uuid> = sqlx::query_scalar("SELECT provider_id FROM ai_generations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(provider, None);
    assert_eq!(rows(&pool).await[0].outcome, "ok");
}

/// Review Focus 3: empty `choices` (`invalid_response`) is invalid output: one repair.
#[sqlx::test(migrations = "../data/migrations")]
async fn empty_choices_once_then_valid_is_ok(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push(Err(FailureReason::InvalidResponse.into()));
    h.provider.push_text(answer(&[(a, "Second try", &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 2);
    assert_eq!(g.draft.post.text, "Second try");
    let repair = h.provider.last_request().unwrap();
    assert!(repair_text(&repair).contains("Problems:"));
    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "ok");
    assert_eq!(
        rows[0].input_tokens,
        Some(100),
        "only the answer that came reported usage"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn empty_choices_twice_is_invalid_output(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    h.provider.push(Err(FailureReason::InvalidResponse.into()));
    h.provider.push(Err(FailureReason::InvalidResponse.into()));

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::ProviderFailed(e) if e.reason == FailureReason::InvalidOutput),
        "{err:?}"
    );
    assert_eq!(h.provider.chat_calls(), 2);
    let rows = rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, "invalid_output");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn length_twice_is_invalid_output(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    for _ in 0..2 {
        h.provider.push(Ok(ChatResponse {
            text: format!("{{\"variants\": [{{\"target\": \"{a}\", \"text\": \"Spr"),
            usage: Some(SCRIPTED_USAGE),
            finish: Finish::Length,
        }));
    }

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DrafterError::ProviderFailed(e) if e.reason == FailureReason::InvalidOutput),
        "{err:?}"
    );
    assert_eq!(h.provider.chat_calls(), 2);
    assert_eq!(rows(&pool).await[0].outcome, "invalid_output");
}

/// Review Focus 5: another owner's provider ID is `ProviderRequired`, not `NotFound`.
#[sqlx::test(migrations = "../data/migrations")]
async fn another_owners_provider_is_provider_required(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let other = seed_user(&pool, "active").await;
    let theirs = match h
        .svc
        .create_provider(&OwnerScope::own(other), openai_preset())
        .await
        .unwrap()
    {
        postit_drafter::ProviderView::Owner { id, .. } => id,
        other @ postit_drafter::ProviderView::Delegate { .. } => panic!("{other:?}"),
    };
    let a = seed_account(&pool, owner, "textish").await;
    let mut req = request(&[a]);
    req.provider_id = Some(theirs);

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), req)
        .await
        .unwrap_err();
    assert!(matches!(err, DrafterError::ProviderRequired), "{err:?}");
    assert_eq!(h.provider.chat_calls(), 0);
}

/// With no language anywhere else, a target is written in the acting user's interface
/// language, not the owner's.
#[sqlx::test(migrations = "../data/migrations")]
async fn locale_falls_back_to_the_actor(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let c = callers(&pool, owner, &[]).await;
    for (user, locale) in [(owner, "fr"), (c.edit.actor, "ar")] {
        sqlx::query("INSERT INTO user_preferences (user_id, locale) VALUES ($1, $2)")
            .bind(user.as_uuid())
            .bind(locale)
            .execute(&pool)
            .await
            .unwrap();
    }
    h.provider.push_text(answer(&[(a, "Hi", &[])]));
    h.svc.generate(&c.edit, request(&[a])).await.unwrap();
    let text = texts(&h.provider.last_request().unwrap(), 0);
    assert!(text.contains("Language: ar"), "{text}");
    assert!(!text.contains("Language: fr"), "{text}");

    // A user with no preferences row falls back to en-US.
    let bare = seed_user(&pool, "active").await;
    h.svc
        .create_provider(&OwnerScope::own(bare), openai_preset())
        .await
        .unwrap();
    let b = seed_account(&pool, bare, "textish").await;
    h.provider.push_text(answer(&[(b, "Hi", &[])]));
    h.svc
        .generate(&OwnerScope::own(bare), request(&[b]))
        .await
        .unwrap();
    assert!(texts(&h.provider.last_request().unwrap(), 0).contains("Language: en-US"));
}

/// An option the account needs is the user's to set, not the model's: no repair.
#[sqlx::test(migrations = "../data/migrations")]
async fn option_violation_does_not_repair(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "optionish").await;
    h.provider.push_text(answer(&[(a, "Hi", &[])]));

    let g = h
        .svc
        .generate(&OwnerScope::own(owner), request(&[a]))
        .await
        .unwrap();
    assert_eq!(h.provider.chat_calls(), 1);
    assert_eq!(codes(&g, a), ["option_required"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn duplicate_media_refused(pool: PgPool) {
    let h = harness(&pool);
    let owner = owner_with_provider(&pool, &h).await;
    let a = seed_account(&pool, owner, "textish").await;
    let photo = seed_media(&pool, owner, "image/jpeg", "ready", None).await;
    let mut req = request(&[a]);
    req.media_ids = vec![photo, photo];

    let err = h
        .svc
        .generate(&OwnerScope::own(owner), req)
        .await
        .unwrap_err();
    let DrafterError::Validation(vs) = &err else {
        panic!("{err:?}");
    };
    assert_eq!(key_of(&vs[0]), "ai.media_duplicate");
    assert_eq!(vs[0].path.as_deref(), Some("media_ids[1]"));
    assert_eq!(h.provider.chat_calls(), 0);
}
