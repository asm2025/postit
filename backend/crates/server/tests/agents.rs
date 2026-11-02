//! The compiled-in agents and the production guard for the mock agent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::path::{Path, PathBuf};

use postit_config::{Environment, Settings, load_with};
use postit_core::{AgentKind, PlatformId};
use postit_server::compose::{agent_registry, refuse_mock_in_production};

fn config_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config")
}

fn development() -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "agents-test-key"),
        (
            "POSTIT__IDENTITY_MAIL__SIGNING_KEY",
            "identity-mail-test-key-0123456789",
        ),
        (
            "POSTIT__PUSH__VAPID_PRIVATE_KEY",
            "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw",
        ),
    ];
    load_with(
        Environment::Development,
        &config_dir(),
        vars.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .unwrap()
}

/// The shipped production config with every value a deployment must supply.
fn production() -> Settings {
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "postgres"),
        ("POSTIT__DATABASE__PASSWORD", "db-password"),
        (
            "POSTIT__AUDIT__PSEUDONYM_KEY",
            "0123456789abcdef0123456789abcdef",
        ),
        (
            "POSTIT__VAULT__KEYS__1",
            "vault-key-0123456789abcdef0123456789abcdef",
        ),
        (
            "POSTIT__MEDIA__SIGNING_KEY",
            "media-key-0123456789abcdef0123456789abcdef",
        ),
        (
            "POSTIT__EMAIL__SIGNING_KEY",
            "email-key-0123456789abcdef0123456789abcdef",
        ),
        ("POSTIT__AUTH__OIDC__ISSUER", "https://auth.example.com"),
        ("POSTIT__AUTH__OIDC__AUDIENCES", "[\"postit\"]"),
        ("POSTIT__MAIL__FROM_ADDRESS", "postit <noreply@example.com>"),
        ("POSTIT__MAIL__SMTP__HOST", "smtp.example.com"),
        ("POSTIT__SERVER__PUBLIC_URL", "https://api.example.com"),
        ("POSTIT__SERVER__WEB__ROOT", "/srv/web"),
        ("POSTIT__APP__PUBLIC_URL", "https://app.example.com"),
        (
            "POSTIT__CORS__ALLOWED_ORIGINS",
            "[\"https://app.example.com\"]",
        ),
    ];
    load_with(
        Environment::Production,
        &config_dir(),
        vars.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .unwrap()
}

#[test]
fn telegram_is_registered() {
    let registry = agent_registry(&development()).unwrap();
    let telegram = registry
        .get(&PlatformId::new("telegram").unwrap())
        .expect("the Telegram agent is registered");
    assert_eq!(telegram.descriptor().kind, AgentKind::Broadcast);
}

#[test]
fn registry_has_both_mocks() {
    let registry = agent_registry(&development()).unwrap();
    for id in ["mock", "mock_direct"] {
        let id = PlatformId::new(id).unwrap();
        assert!(registry.get(&id).is_some(), "{id} is registered");
    }
}

/// Only the registry: the self dev-dependency compiles the mock into every server test, so
/// `refuse_mock_in_production` would refuse a real production start here.
#[test]
fn registry_for_production_settings_holds_email() {
    let settings = production();
    assert!(!settings.email.allow_private_hosts);
    let registry = agent_registry(&settings).unwrap();
    let email = registry
        .get(&PlatformId::new("email").unwrap())
        .expect("the email agent is registered");
    assert_eq!(email.descriptor().kind, AgentKind::Direct);
}

#[test]
fn production_refuses_mock() {
    let err = refuse_mock_in_production(Environment::Production).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the mock agent is compiled in; production builds must not enable the mock-agent feature"
    );
    assert!(refuse_mock_in_production(Environment::Development).is_ok());
    assert!(refuse_mock_in_production(Environment::Qa).is_ok());
}
