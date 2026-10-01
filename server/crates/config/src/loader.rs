use std::path::Path;

use figment::providers::{Format, Toml};
use figment::value::{Dict, Map, Tag, Value};
use figment::{Figment, Metadata, Profile, Provider};

use crate::environment::Environment;
use crate::error::ConfigError;
use crate::settings::Settings;

const ENV_PREFIX: &str = "POSTIT__";
const FILE_SUFFIX: &str = "_FILE";
const SECRETS_FILE_VAR: &str = "POSTIT_SECRETS_FILE";

/// Loads and validates `Settings` for `env` from `config_dir`, applying the layering
/// documented in plan 02: `default.toml`, `{env}.toml`, `local.toml` (development only),
/// the `POSTIT_SECRETS_FILE` dotenv file, `POSTIT__SECTION__KEY` env vars, then their
/// `_FILE` counterparts.
///
/// # Errors
///
/// Returns a [`ConfigError`] when a config file can't be read, a value doesn't match the
/// `Settings` schema (including unknown keys), or [`Settings::validate`] rejects the result.
pub fn load(env: Environment, config_dir: &Path) -> Result<Settings, ConfigError> {
    load_with(env, config_dir, std::env::vars())
}

/// Like [`load`], with the environment variables supplied explicitly.
///
/// # Errors
///
/// The same as [`load`].
pub fn load_with(
    env: Environment,
    config_dir: &Path,
    vars: impl IntoIterator<Item = (String, String)>,
) -> Result<Settings, ConfigError> {
    let vars: Vec<(String, String)> = vars.into_iter().collect();
    let figment = build_figment(env, config_dir, &vars);
    let settings: Settings = figment.extract()?;
    settings.validate(env)?;
    Ok(settings)
}

fn build_figment(env: Environment, config_dir: &Path, vars: &[(String, String)]) -> Figment {
    let mut figment = Figment::new()
        .merge(Toml::file(config_dir.join("default.toml")))
        .merge(Toml::file(config_dir.join(format!("{env}.toml"))));

    if env == Environment::Development {
        figment = figment.merge(Toml::file(config_dir.join("local.toml")));
    }

    if let Some((_, path)) = vars.iter().find(|(key, _)| key == SECRETS_FILE_VAR) {
        figment = figment.merge(SecretsFile(path.clone()));
    }

    figment
        .merge(EnvVars {
            vars: vars.to_vec(),
            lists: true,
        })
        .merge(FileSecrets(vars.to_vec()))
}

/// The file named by `POSTIT_SECRETS_FILE`: one environment's vault file
/// (`!ref/vault/<environment>/postit.env`), holding `POSTIT__SECTION__KEY=value` lines in
/// the same format Compose reads through `env_file:`. It lets a natively run server read
/// the same secrets a container gets as env vars; real env vars still override it.
struct SecretsFile(String);

impl Provider for SecretsFile {
    fn metadata(&self) -> Metadata {
        Metadata::named(format!("secrets file {} ({SECRETS_FILE_VAR})", self.0))
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let content = std::fs::read_to_string(&self.0).map_err(|err| -> figment::Error {
            format!("reading {SECRETS_FILE_VAR} {}: {err}", self.0).into()
        })?;
        EnvVars {
            vars: parse_dotenv(&content),
            lists: false,
        }
        .data()
    }
}

/// `KEY=value` lines; blank lines and `#` comments are skipped, and one pair of
/// matching quotes around a value is removed, as Compose's `env_file:` parser does.
fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.trim();
            let unquoted = ['"', '\'']
                .iter()
                .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
                .unwrap_or(value);
            (key.trim().to_string(), unquoted.to_string())
        })
        .collect()
}

/// `POSTIT__SECTION__KEY=value` entries, split into the nested settings path they
/// override. Keys ending in `_FILE` are left to [`FileSecrets`], which wins over both
/// this layer and the file layers.
struct EnvVars {
    vars: Vec<(String, String)>,
    /// Real env vars accept `[a, b]` lists; secrets-file values are opaque strings.
    lists: bool,
}

impl Provider for EnvVars {
    fn metadata(&self) -> Metadata {
        Metadata::named("environment variables (POSTIT__*)")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();

        for (key, raw) in &self.vars {
            let Some(rest) = key.strip_prefix(ENV_PREFIX) else {
                continue;
            };
            if rest.ends_with(FILE_SUFFIX) {
                continue;
            }

            let segments: Vec<String> = rest.split("__").map(str::to_lowercase).collect();
            let value = if self.lists {
                coerce_value(raw)
            } else {
                coerce_scalar(raw)
            };
            insert_nested(&mut dict, &segments, value);
        }

        Ok(Map::from([(Profile::Default, dict)]))
    }
}

/// Every `POSTIT__…__KEY_FILE` entry, inserted from its file contents at the path
/// `POSTIT__…__KEY` would occupy, so it overrides both the plain env var and the file layers.
struct FileSecrets(Vec<(String, String)>);

impl Provider for FileSecrets {
    fn metadata(&self) -> Metadata {
        Metadata::named("environment variable file secrets (POSTIT__*_FILE)")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();

        for (key, path) in &self.0 {
            let Some(rest) = key.strip_prefix(ENV_PREFIX) else {
                continue;
            };
            let Some(rest) = rest.strip_suffix(FILE_SUFFIX) else {
                continue;
            };

            let content = std::fs::read_to_string(path)
                .map_err(|err| -> figment::Error {
                    format!("reading secret file {path} for {key}: {err}").into()
                })?
                .trim()
                .to_string();

            let segments: Vec<String> = rest.split("__").map(str::to_lowercase).collect();
            insert_nested(&mut dict, &segments, Value::from(content));
        }

        Ok(Map::from([(Profile::Default, dict)]))
    }
}

/// A bracketed value (`[a, b]`) becomes an array of scalars; anything else is one scalar.
fn coerce_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let items: Vec<Value> = inner
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| {
                let unquoted = ['"', '\'']
                    .iter()
                    .find_map(|q| item.strip_prefix(*q)?.strip_suffix(*q))
                    .unwrap_or(item);
                coerce_scalar(unquoted)
            })
            .collect();
        return Value::from(items);
    }
    coerce_scalar(raw)
}

fn coerce_scalar(raw: &str) -> Value {
    if let Ok(b) = raw.parse::<bool>() {
        return Value::from(b);
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        return Value::from(f);
    }
    Value::from(raw.to_string())
}

fn insert_nested(dict: &mut Dict, segments: &[String], value: Value) {
    let [head, tail @ ..] = segments else {
        return;
    };

    if tail.is_empty() {
        dict.insert(head.clone(), value);
        return;
    }

    let entry = dict
        .entry(head.clone())
        .or_insert_with(|| Value::Dict(Tag::Default, Dict::new()));

    if let Value::Dict(_, nested) = entry {
        insert_nested(nested, tail, value);
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    /// A minimal but complete config satisfying every required field in `Settings`.
    const BASELINE: &str = r#"
[server]
host = "0.0.0.0"
api_port = 44310
worker_port = 44311
public_url = "https://postit.local:44310"
shutdown_timeout = "10s"
[server.tls]
enabled = false
[server.web]
enabled = false

[database]
url = "postgres://localhost/postit"
username = "postit"
password = "baseline-db-password"
max_connections = 10

[auth.oidc]
issuer = "https://postit.local:44300"
audiences = ["postit"]
client_id = "postit-app"
scopes = ["openid"]
accepted_algorithms = ["RS256"]
leeway = "1min"
jwks_refresh_interval = "1h"
userinfo = "fallback"
[auth.oidc.claim_names]
email = "email"
email_verified = "email_verified"
name = "name"
preferred_username = "preferred_username"
[auth.bootstrap]
admin_email = "admin@postit.com"
[auth]
pending_ttl = "30days"
principal_cache_ttl = "5min"
approval_email_interval = "1h"

[audit]
retention = "365days"
ip_retention = "90days"
pseudonym_key = "baseline-pseudonym-key"

[retention]
notifications_read_after = "90days"
delivery_attempts_after = "180days"
ai_generation_prompt_after = "30days"
ai_generation_row_after = "365days"

[ops]
check_interval = "5min"
due_delivery_overdue_after = "5min"

[rate_limit.unauthenticated]
rate_per_minute = 60
burst = 20
[rate_limit.provisioning]
rate_per_minute = 10
burst = 5
[rate_limit.authenticated]
rate_per_minute = 300
burst = 60

[mail]
transport = "smtp"
from_address = "noreply@postit.com"
send_email_max_attempts = 8
[mail.smtp]
host = "localhost"
port = 25
tls = "none"

[jobs]
outbox_poll_interval = "5s"
[jobs.concurrency]
mail = 4
maintenance = 1
default = 4
[jobs.history_retention]
succeeded = "7days"
failed = "30days"
[jobs.schedules]
job_history_purge = "0 10 3 * * *"
purge_pending_users = "0 20 3 * * *"
audit_retention = "0 30 3 * * *"
data_retention = "0 40 3 * * *"

[http]
connect_timeout = "5s"
request_timeout = "30s"
user_agent = "postit/test"

[app]
public_url = "https://postit.local:44315"

[cors]
allowed_origins = ["https://postit.local:44315"]
"#;

    fn write(dir: &std::path::Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap_or_default();
    }

    fn open_tempdir() -> tempfile::TempDir {
        tempdir().unwrap_or_else(|err| {
            eprintln!("could not create tempdir: {err}");
            std::process::exit(97)
        })
    }

    fn ok_settings(result: Result<Settings, ConfigError>) -> Settings {
        result.unwrap_or_else(|err| {
            eprintln!("unexpected config error: {err}");
            std::process::exit(98)
        })
    }

    #[test]
    fn loads_a_complete_baseline() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(settings.server.api_port, 44310);
    }

    #[test]
    fn per_environment_file_overrides_default() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[server]\napi_port = 55000\n",
        );

        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(settings.server.api_port, 55000);
    }

    #[test]
    fn local_toml_overrides_the_environment_file() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[server]\napi_port = 55000\n",
        );
        write(dir.path(), "local.toml", "[server]\napi_port = 60000\n");

        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(settings.server.api_port, 60000);
    }

    #[test]
    fn env_var_overrides_every_file_layer() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[server]\napi_port = 55000\n",
        );
        write(dir.path(), "local.toml", "[server]\napi_port = 60000\n");

        let vars = [("POSTIT__SERVER__API_PORT".to_string(), "61000".to_string())];
        let settings = ok_settings(load_with(Environment::Development, dir.path(), vars));
        assert_eq!(settings.server.api_port, 61000);
    }

    #[test]
    fn file_secret_overrides_the_plain_env_var() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let secret_dir = open_tempdir();
        let secret_path = secret_dir.path().join("pseudonym_key");
        std::fs::write(&secret_path, "from-file-secret\n").unwrap_or_default();
        let secret_path = secret_path.to_string_lossy().to_string();

        let vars = [
            (
                "POSTIT__AUDIT__PSEUDONYM_KEY".to_string(),
                "from-plain-env-var".to_string(),
            ),
            ("POSTIT__AUDIT__PSEUDONYM_KEY_FILE".to_string(), secret_path),
        ];
        let settings = ok_settings(load_with(Environment::Development, dir.path(), vars));

        let key = settings.audit.pseudonym_key.expose();
        assert_eq!(key, "from-file-secret");
    }

    #[test]
    fn secrets_file_fills_in_secrets_and_env_vars_override_it() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let vault_dir = open_tempdir();
        let secrets_path = vault_dir.path().join("postit.env");
        std::fs::write(
            &secrets_path,
            "# comment

POSTIT__DATABASE__PASSWORD=from-secrets-file
             POSTIT__AUDIT__PSEUDONYM_KEY=\"quoted-key\"
             POSTIT__SERVER__API_PORT=55000
",
        )
        .unwrap_or_default();

        let vars = [
            (
                "POSTIT_SECRETS_FILE".to_string(),
                secrets_path.to_string_lossy().to_string(),
            ),
            ("POSTIT__SERVER__API_PORT".to_string(), "61000".to_string()),
        ];
        let settings = ok_settings(load_with(Environment::Development, dir.path(), vars));

        assert_eq!(settings.database.password.expose(), "from-secrets-file");
        let key = settings.audit.pseudonym_key.expose();
        assert_eq!(key, "quoted-key");
        assert_eq!(settings.server.api_port, 61000);
    }

    #[test]
    fn missing_secrets_file_is_an_error() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let missing = dir.path().join("no-such-postit.env");
        let vars = [(
            "POSTIT_SECRETS_FILE".to_string(),
            missing.to_string_lossy().to_string(),
        )];
        let result = load_with(Environment::Development, dir.path(), vars);
        assert!(result.is_err());
    }

    #[test]
    fn database_url_with_credentials_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[database]
url = \"postgres://user:pass@localhost/postit\"
",
        );

        let result = load_with(Environment::Development, dir.path(), []);
        assert!(matches!(result, Err(ConfigError::Validation(_))));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[server]\nnot_a_real_field = true\n",
        );

        let result = load_with(Environment::Development, dir.path(), []);
        assert!(result.is_err());
    }

    #[test]
    fn redacted_dump_hides_secrets() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        let dump = settings.redacted_dump().unwrap_or(serde_json::Value::Null);
        let rendered = dump.to_string();

        assert!(!rendered.contains("baseline-pseudonym-key"));
        assert!(!rendered.contains("baseline-db-password"));
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn invalid_value_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[server]\napi_port = \"not-a-number\"\n",
        );

        let result = load_with(Environment::Development, dir.path(), []);
        assert!(result.is_err());
    }

    #[test]
    fn missing_pseudonym_key_fails_in_development_too() {
        let dir = open_tempdir();
        let without_key = BASELINE.replace("pseudonym_key = \"baseline-pseudonym-key\"\n", "");
        write(dir.path(), "default.toml", &without_key);
        write(dir.path(), "development.toml", "");

        let result = load_with(
            Environment::Development,
            dir.path(),
            Vec::<(String, String)>::new(),
        );
        let Err(err) = result else {
            unreachable!("config without audit.pseudonym_key unexpectedly loaded");
        };
        assert!(err.to_string().contains("pseudonym_key"), "{err}");
    }

    #[test]
    fn smtp_tls_none_is_rejected_outside_development() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "qa.toml", "");

        let result = load_with(Environment::Qa, dir.path(), Vec::<(String, String)>::new());
        let Err(err) = result else {
            unreachable!("qa config with mail.smtp.tls = none unexpectedly loaded");
        };
        assert!(err.to_string().contains("mail.smtp.tls"), "{err}");
    }

    #[test]
    fn jobs_schedules_and_mail_budget_load() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let settings = ok_settings(load_with(
            Environment::Development,
            dir.path(),
            Vec::<(String, String)>::new(),
        ));
        assert_eq!(settings.jobs.schedules.purge_pending_users, "0 20 3 * * *");
        assert_eq!(settings.mail.send_email_max_attempts, 8);
        assert_eq!(settings.mail.smtp.tls, crate::SmtpTls::None);
    }

    #[test]
    fn zero_duration_is_rejected_with_its_key() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[audit]\nretention = \"0s\"\n",
        );
        let err = load_with(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("audit.retention")),
            "got {err:?}"
        );
    }

    #[test]
    fn duration_over_one_hundred_years_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[auth]\npending_ttl = \"200years\"\n",
        );
        let err = load_with(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("auth.pending_ttl")),
            "got {err:?}"
        );
    }

    #[test]
    fn zero_leeway_is_allowed() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[auth.oidc]\nleeway = \"0s\"\n",
        );
        assert!(load_with(Environment::Development, dir.path(), []).is_ok());
    }

    #[test]
    fn zero_rate_limit_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[rate_limit.authenticated]\nrate_per_minute = 0\nburst = 1\n",
        );
        let err = load_with(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("rate_limit.authenticated")),
            "got {err:?}"
        );
    }

    #[test]
    fn request_timeout_and_body_limit_default() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");
        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(
            settings.server.request_timeout,
            std::time::Duration::from_secs(30)
        );
        assert_eq!(settings.server.body_limit, 1_048_576);
    }

    #[test]
    fn bracketed_env_value_becomes_a_list() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");
        let settings = ok_settings(load_with(
            Environment::Development,
            dir.path(),
            [
                (
                    "POSTIT__SERVER__TRUSTED_PROXIES".to_string(),
                    "[172.30.0.0/24, \"10.0.0.1\"]".to_string(),
                ),
                (
                    "POSTIT__HTTP__EXTRA_CA_FILES".to_string(),
                    "[/certs/ca.crt]".to_string(),
                ),
            ],
        ));
        assert_eq!(
            settings.server.trusted_proxies,
            vec!["172.30.0.0/24", "10.0.0.1"]
        );
        assert_eq!(
            settings.http.extra_ca_files,
            vec![std::path::PathBuf::from("/certs/ca.crt")]
        );
    }

    #[test]
    fn shipped_config_files_validate() {
        // server/config/*.toml must keep loading after this task's new rules. Secrets are
        // supplied inline so no vault is needed.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let secrets = [
            ("POSTIT__DATABASE__USERNAME".to_string(), "u".to_string()),
            ("POSTIT__DATABASE__PASSWORD".to_string(), "p".to_string()),
            ("POSTIT__AUDIT__PSEUDONYM_KEY".to_string(), "k".to_string()),
        ];
        ok_settings(load_with(Environment::Development, &dir, secrets.clone()));
        ok_settings(load_with(Environment::Qa, &dir, secrets.clone()));
    }

    #[test]
    fn secrets_file_values_are_never_split_into_lists() {
        // Only real env vars get the `[a, b]` list form; a secret that happens to start with
        // `[` must reach its setting verbatim.
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");
        let secrets = dir.path().join("postit.env");
        std::fs::write(&secrets, "POSTIT__DATABASE__PASSWORD=[abc]\n")
            .unwrap_or_else(|e| unreachable!("write secrets file: {e}"));
        let settings = ok_settings(load_with(
            Environment::Development,
            dir.path(),
            [(
                "POSTIT_SECRETS_FILE".to_string(),
                secrets.display().to_string(),
            )],
        ));
        assert_eq!(settings.database.password.expose(), "[abc]");
    }
}
