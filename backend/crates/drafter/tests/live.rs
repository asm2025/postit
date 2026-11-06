//! One real `post`-mode draft from Google Gemini's free tier, through the preset, the real
//! OpenAI-compatible protocol, and the strict address policy, checked the way the service
//! checks a draft.
//!
//! It runs only with `POSTIT_LIVE_TESTS=1` and a key in `POSTIT_LIVE_GEMINI_KEY`, and only
//! when asked for: `cargo test -p postit-drafter --test live -- --ignored`. The model is
//! `POSTIT_LIVE_GEMINI_MODEL` when set, else `gemini-2.5-flash`. CI never runs it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use postit_config::HttpSettings;
use postit_core::{
    AccountId, AccountRef, EmailLanguage, Schedule, TextMeasure, ValidationInput, resolve_variant,
    validate_target,
};
use postit_drafter::output::{parse, to_draft};
use postit_drafter::prompt::{
    PromptInput, TargetBrief, output_schema, system_prompt, user_message,
};
use postit_drafter::testkit::text_descriptor;
use postit_drafter::{
    ChatMessage, ChatRequest, Finish, HttpProviderFactory, Mode, OutputMode, Part, ProviderConfig,
    ProviderFactory, ProviderSecret, Role, preset,
};
use postit_http::{AddressPolicy, build_public_client};
use secrecy::SecretString;
use tokio::time::Instant;
use uuid::Uuid;

const PLATFORM: &str = "textish";
const TEXT_MAX: u32 = 280;
const DEFAULT_MODEL: &str = "gemini-2.5-flash";

#[tokio::test]
#[ignore = "calls Google Gemini with a real key; set POSTIT_LIVE_TESTS=1"]
async fn live_gemini_post_mode() {
    if std::env::var("POSTIT_LIVE_TESTS").as_deref() != Ok("1") {
        return;
    }
    let key = std::env::var("POSTIT_LIVE_GEMINI_KEY").expect("POSTIT_LIVE_GEMINI_KEY");
    let model =
        std::env::var("POSTIT_LIVE_GEMINI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());

    let client = build_public_client(
        &HttpSettings {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(120),
            user_agent: "postit-drafter-live/0".to_owned(),
            extra_ca_files: Vec::<PathBuf>::new(),
        },
        AddressPolicy::strict(),
    )
    .unwrap();
    let gemini = preset("gemini").unwrap();
    let config = ProviderConfig {
        protocol: Some(gemini.protocol),
        default_model: Some(model.clone()),
        ..ProviderConfig::default()
    };
    let secret = ProviderSecret {
        api_key: Some(SecretString::from(key)),
        headers: std::collections::BTreeMap::new(),
    };
    let provider = HttpProviderFactory::new(client)
        .build(&config, secret, Some(gemini))
        .unwrap();

    let descriptor = text_descriptor(PLATFORM, TEXT_MAX, TextMeasure::Graphemes);
    let account = AccountId::from_uuid(Uuid::now_v7());
    let briefs = [TargetBrief {
        account,
        platform_name: descriptor.display_name.clone(),
        is_email: false,
        text_max: TEXT_MAX,
        measure: TextMeasure::Graphemes,
        includes_link_and_tags: false,
        style: None,
        language: Some("en-US".to_owned()),
    }];
    let input = PromptInput {
        mode: Mode::Post,
        user_prompt: "Announce that our bakery's spring menu starts on Monday, with lemon \
                      tarts and rhubarb buns.",
        link: None,
        brand: None,
        targets: &briefs,
        reply: None,
        fallback_language: "en-US",
        media_note: None,
    };
    let max_tokens = config.max_tokens(&model);
    let request = ChatRequest {
        model,
        system: system_prompt(&input),
        messages: vec![ChatMessage {
            role: Role::User,
            parts: vec![Part::Text(user_message(&input))],
        }],
        temperature: None,
        // The default leaves room for the model's thinking before its answer.
        max_tokens,
        output: OutputMode::JsonSchema {
            name: "drafts".to_owned(),
            schema: output_schema(&briefs),
        },
        deadline: Instant::now() + Duration::from_secs(120),
    };

    let answer = provider.chat(&request).await.unwrap();
    assert_eq!(answer.finish, Finish::Stop, "{answer:?}");
    let usage = answer.usage.expect("Gemini reports usage");
    assert!(
        usage.input_tokens > 0 && usage.output_tokens > 0,
        "{usage:?}"
    );

    let parsed = parse(&answer.text, &briefs).unwrap();
    assert!(parsed.missing.is_empty(), "the one target is answered");
    let draft = to_draft(&parsed, &briefs, &[], None, None, |_| EmailLanguage::EnUs);
    let target = AccountRef {
        platform: descriptor.platform.clone(),
        account,
        remote_id: "live".to_owned(),
    };
    let variant = resolve_variant(&draft.post, &target, draft.overrides.get(&account)).unwrap();
    assert!(!variant.text.trim().is_empty(), "the draft has text");
    assert!(variant.disclose_ai_generated);
    let violations = validate_target(&ValidationInput {
        descriptor: &descriptor,
        options_schema: &descriptor.options_schema,
        variant: &variant,
        media: &[],
        schedule: Schedule::Now,
        now: Utc::now(),
    });
    assert!(violations.is_empty(), "{violations:?}");
}
