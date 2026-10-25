//! The effective-config line logged at startup carries no secret. Own test binary: it
//! captures logs.

use postit_config::{Environment, load_with};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};

#[test]
fn the_startup_config_dump_carries_no_secret() {
    let (logs, _guard) = LogCapture::install();
    let secrets = [
        "db-pass-91c2",
        "smtp-pass-44d1",
        "pseudonym-key-0a7e",
        "identity-mail-key-5b3f",
    ];
    let vars = vec![
        (
            "POSTIT__DATABASE__USERNAME".to_string(),
            "postgres".to_string(),
        ),
        (
            "POSTIT__DATABASE__PASSWORD".to_string(),
            secrets[0].to_string(),
        ),
        (
            "POSTIT__MAIL__SMTP__USERNAME".to_string(),
            "mailer".to_string(),
        ),
        (
            "POSTIT__MAIL__SMTP__PASSWORD".to_string(),
            secrets[1].to_string(),
        ),
        (
            "POSTIT__AUDIT__PSEUDONYM_KEY".to_string(),
            secrets[2].to_string(),
        ),
        (
            "POSTIT__IDENTITY_MAIL__SIGNING_KEY".to_string(),
            secrets[3].to_string(),
        ),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let settings =
        load_with(Environment::Development, &dir, vars).unwrap_or_else(|e| unreachable!("{e}"));
    postit_server::log_effective_config(&settings);
    let logs = logs.text();
    assert!(logs.contains("effective configuration"), "{logs}");
    assert_no_leak(
        &secrets,
        &[("logs", &logs), ("debug", &format!("{settings:?}"))],
    );
}
