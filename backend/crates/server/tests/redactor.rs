//! The deployment redactor built at startup covers the built-in webhook hosts and the
//! deployment's own signed media and email tracking routes.

use postit_config::{Environment, Settings, load_with};
use postit_server::compose;
use url::Url;

fn settings() -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "postgres"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "unused"),
        // Development enables the identity mail webhook, which needs its key.
        ("POSTIT__IDENTITY_MAIL__SIGNING_KEY", "unused-webhook-key"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
    load_with(Environment::Development, &dir, vars).unwrap_or_else(|e| unreachable!("{e}"))
}

fn url(raw: &str) -> Url {
    Url::parse(raw).unwrap_or_else(|e| unreachable!("{raw}: {e}"))
}

#[test]
fn falls_back_to_the_server_public_url() {
    let mut settings = settings();
    settings.server.public_url = url("https://api.postit.test/");
    settings.media.public_base_url = None;
    settings.email.tracking_base_url = None;
    let redactor = compose::redactor(&settings);

    for raw in [
        "https://api.postit.test/public/media/abc-SECRET?exp=1&sig=zz",
        "https://API.postit.test./public/email/open/SECRET",
        "https://hooks.slack.com/services/T/B/SECRET",
        "https://discord.com/api/webhooks/1/SECRET",
    ] {
        let text = redactor.redact_url(&url(raw));
        assert!(!text.contains("SECRET"), "{raw} -> {text}");
    }
    let api = "https://api.postit.test/api/v1/posts";
    assert_eq!(redactor.redact_url(&url(api)), api);
}

#[test]
fn uses_the_media_and_tracking_bases_with_their_paths() {
    let mut settings = settings();
    settings.server.public_url = url("https://api.postit.test/");
    settings.media.public_base_url = Some(url("https://media.postit.test/m"));
    settings.email.tracking_base_url = Some(url("https://t.postit.test/e/"));
    let redactor = compose::redactor(&settings);

    for raw in [
        "https://media.postit.test/m/public/media/SECRET",
        "https://t.postit.test/e/public/unsubscribe/SECRET",
    ] {
        let text = redactor.redact_url(&url(raw));
        assert!(!text.contains("SECRET"), "{raw} -> {text}");
    }
    // The server's own `/public/` is not a sensitive route once both bases are set elsewhere.
    let other = "https://api.postit.test/public/health";
    assert_eq!(redactor.redact_url(&url(other)), other);
}
