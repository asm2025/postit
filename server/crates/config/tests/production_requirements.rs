//! Plan 02 P9: production refuses to start without an OIDC issuer and audience, a
//! database password, SMTP settings, and `audit.pseudonym_key`. The bootstrap-admin
//! requirement needs the database and is covered by `postit-identity`'s
//! `bootstrap::check_startup` tests.

mod common;

use postit_config::{Environment, load_with};

use common::{production_vars, shipped_config_dir};

fn load(
    vars: Vec<(String, String)>,
) -> Result<postit_config::Settings, postit_config::ConfigError> {
    load_with(Environment::Production, &shipped_config_dir(), vars)
}

fn without(key: &str) -> Vec<(String, String)> {
    production_vars()
        .into_iter()
        .filter(|(k, _)| k != key)
        .collect()
}

fn with(key: &str, value: &str) -> Vec<(String, String)> {
    let mut vars = without(key);
    vars.push((key.to_owned(), value.to_owned()));
    vars
}

fn assert_refused(vars: Vec<(String, String)>, mentions: &str) {
    let Err(err) = load(vars) else {
        unreachable!("production config unexpectedly loaded; expected an error about {mentions}");
    };
    assert!(err.to_string().contains(mentions), "{err}");
}

#[test]
fn complete_production_config_loads() {
    let settings = load(production_vars()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        settings.auth.oidc.issuer.as_str(),
        "https://auth.example.com/"
    );
}

#[test]
fn missing_issuer_is_refused() {
    assert_refused(without("POSTIT__AUTH__OIDC__ISSUER"), "issuer");
}

#[test]
fn missing_audience_is_refused() {
    assert_refused(without("POSTIT__AUTH__OIDC__AUDIENCES"), "audiences");
}

#[test]
fn empty_audience_list_is_refused() {
    assert_refused(with("POSTIT__AUTH__OIDC__AUDIENCES", "[]"), "audiences");
}

#[test]
fn missing_database_password_is_refused() {
    assert_refused(without("POSTIT__DATABASE__PASSWORD"), "password");
}

#[test]
fn blank_database_password_is_refused() {
    assert_refused(with("POSTIT__DATABASE__PASSWORD", ""), "database.password");
}

#[test]
fn missing_smtp_host_is_refused() {
    assert_refused(without("POSTIT__MAIL__SMTP__HOST"), "host");
}

#[test]
fn blank_smtp_host_is_refused() {
    assert_refused(with("POSTIT__MAIL__SMTP__HOST", " "), "mail.smtp.host");
}

#[test]
fn missing_from_address_is_refused() {
    assert_refused(without("POSTIT__MAIL__FROM_ADDRESS"), "from_address");
}

#[test]
fn missing_pseudonym_key_is_refused() {
    assert_refused(without("POSTIT__AUDIT__PSEUDONYM_KEY"), "pseudonym_key");
}

#[test]
fn blank_pseudonym_key_is_refused() {
    assert_refused(
        with("POSTIT__AUDIT__PSEUDONYM_KEY", ""),
        "audit.pseudonym_key",
    );
}

#[test]
fn blank_bootstrap_email_is_no_rule() {
    let settings = load(with("POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL", ""))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(settings.auth.bootstrap.admin_email, None);
}

#[test]
fn bootstrap_email_is_kept() {
    let settings = load(with(
        "POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL",
        "admin@example.com",
    ))
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        settings.auth.bootstrap.admin_email.as_deref(),
        Some("admin@example.com")
    );
}
