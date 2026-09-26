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
    load_with_vars(env, config_dir, std::env::vars())
}

fn load_with_vars(
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
        .merge(EnvVars(vars.to_vec()))
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
        EnvVars(parse_dotenv(&content)).data()
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
struct EnvVars(Vec<(String, String)>);

impl Provider for EnvVars {
    fn metadata(&self) -> Metadata {
        Metadata::named("environment variables (POSTIT__*)")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();

        for (key, raw) in &self.0 {
            let Some(rest) = key.strip_prefix(ENV_PREFIX) else {
                continue;
            };
            if rest.ends_with(FILE_SUFFIX) {
                continue;
            }

            let segments: Vec<String> = rest.split("__").map(str::to_lowercase).collect();
            insert_nested(&mut dict, &segments, coerce_scalar(raw));
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
api_port = 44300
worker_port = 44305
public_url = "https://postit.local:44300"
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
issuer = "https://postit.local:44330"
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
[mail.smtp]
host = "localhost"
port = 44325
starttls = false

[jobs]
outbox_poll_interval = "5s"
[jobs.history_retention]
succeeded = "7days"
failed = "30days"

[http]
connect_timeout = "5s"
request_timeout = "30s"
user_agent = "postit/test"

[app]
public_url = "https://postit.local:44310"

[cors]
allowed_origins = ["https://postit.local:44310"]
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

        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), []));
        assert_eq!(settings.server.api_port, 44300);
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

        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), []));
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

        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), []));
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
        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), vars));
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
        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), vars));

        let key = settings
            .audit
            .pseudonym_key
            .as_ref()
            .map(crate::RedactedSecret::expose)
            .unwrap_or_default();
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
        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), vars));

        assert_eq!(settings.database.password.expose(), "from-secrets-file");
        let key = settings
            .audit
            .pseudonym_key
            .as_ref()
            .map(crate::RedactedSecret::expose)
            .unwrap_or_default();
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
        let result = load_with_vars(Environment::Development, dir.path(), vars);
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

        let result = load_with_vars(Environment::Development, dir.path(), []);
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

        let result = load_with_vars(Environment::Development, dir.path(), []);
        assert!(result.is_err());
    }

    #[test]
    fn redacted_dump_hides_secrets() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");

        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), []));
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

        let result = load_with_vars(Environment::Development, dir.path(), []);
        assert!(result.is_err());
    }
}
