//! `cargo xtask zitadel-bootstrap`, run from `server/`: waits for the dev Zitadel
//! container, creates the `postit` project, the `postit-app` web application, the
//! `postit-android` and `postit-ios` native applications, and the `member@postit.com`
//! user, sets the login policy (registration from `ZITADEL_ALLOW_REGISTRATION`, default
//! allowed; a forced second factor from `ZITADEL_FORCE_MFA`, default off in development
//! and on in qa and production, per `POSTIT_ENV`), then writes the generated client ids
//! and audiences into `config/local.toml`.
//! Mobile tokens carry the project ID in `aud`, so the audiences need no mobile entry. Every step is idempotent, so re-running after `stack down`
//! and back `up` is safe. Machine-user auth uses the JWT profile (RFC 7523) against the
//! machine key Zitadel prints once during `FirstInstance` setup (see
//! `docker/zitadel/steps.yaml`), which this task captures from `docker logs postit-zitadel`
//! on its first run and caches at `../docker/zitadel/machinekey/postit-bootstrap.json`.
//! It also installs postit's English and Arabic identity message texts, registers the
//! HTTP email provider that sends Zitadel's mail to postit's webhook (writing its signing
//! key to the development vault), and applies postit's branding (plan 03 D0c). Every
//! mutating call is printed. `--check` only reports and fails when the webhook provider
//! is not the active one.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use postit_config::{Environment, HttpSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::zitadel_messages;

const ISSUER: &str = "https://postit.local:44300";
const CA_PATH: &str = "../docker/shared/nginx/certs/postit-dev-ca.crt";
const MACHINE_KEY_PATH: &str = "../docker/zitadel/machinekey/postit-bootstrap.json";
/// Pinned by `container_name` in `docker/docker-compose.development.yml`.
const ZITADEL_CONTAINER: &str = "postit-zitadel";
const LOCAL_TOML_PATH: &str = "config/local.toml";
const PROJECT_NAME: &str = "postit";
const WEB_APP: &str = "postit-app";
const ANDROID_APP: &str = "postit-android";
const IOS_APP: &str = "postit-ios";
/// The React web app, then Swagger UI's `oauth2-redirect.html` (plan 02 P3).
const WEB_REDIRECT_URIS: [&str; 2] = [
    "https://postit.local:44315/auth/callback",
    "https://postit.local:44310/docs/oauth2-redirect.html",
];
const WEB_POST_LOGOUT_REDIRECT_URIS: [&str; 1] = ["https://postit.local:44315/"];
/// The development flavor's custom scheme (plan 02 P11); qa and production use their own.
const MOBILE_REDIRECT_URIS: [&str; 1] = ["postit-dev://auth/callback"];
const MOBILE_POST_LOGOUT_REDIRECT_URIS: [&str; 1] = ["postit-dev://auth/logout"];

const MESSAGES_DIR: &str = "../docker/zitadel/messages";
const MESSAGE_LANGUAGES: [&str; 2] = ["en", "ar"];
const BRANDING_LOGO_PATH: &str = "../web/public/postit.svg";
const BRANDING_FONT_PATH: &str = "../docker/zitadel/branding/Onest.ttf";
const IDENTITY_MAIL_DESCRIPTION: &str = "postit: identity mail webhook";
const DEFAULT_IDENTITY_MAIL_URL: &str = "http://host.docker.internal:44312/internal/identity-mail";
const VAULT_POSTIT_ENV: &str = "../!ref/vault/development/postit.env";
const SIGNING_KEY_VAR: &str = "POSTIT__IDENTITY_MAIL__SIGNING_KEY";

const MEMBER_EMAIL: &str = "member@postit.com";
const MEMBER_PASSWORD: &str = "P@$$w0rd";

#[derive(Deserialize)]
struct MachineKey {
    #[serde(rename = "userId")]
    user_id: String,
    #[serde(rename = "keyId")]
    key_id: String,
    key: String,
}

#[derive(Serialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
}

pub async fn run(check_only: bool) -> Result<()> {
    load_env_file(Path::new("../docker/.env"))?;
    let allow_register =
        allow_registration(std::env::var("ZITADEL_ALLOW_REGISTRATION").ok().as_deref())?;
    let environment = Environment::resolve(true)?;
    let mfa = force_mfa(
        std::env::var("ZITADEL_FORCE_MFA").ok().as_deref(),
        environment,
    )?;
    let client = trusted_client()?;

    println!("Waiting for Zitadel at {ISSUER} ...");
    wait_for_discovery(&client).await?;

    let machine_key = load_or_capture_machine_key()?;
    let access_token = machine_bearer_token(&client, &machine_key).await?;

    if check_only {
        return check_identity_mail(&client, &access_token).await;
    }

    let project_id = ensure_project(&client, &access_token).await?;
    ensure_builtin_login(&client, &access_token).await?;
    ensure_login_policy(&client, &access_token, allow_register, mfa).await?;
    ensure_smtp_provider(&client, &access_token).await?;
    ensure_message_texts(&client, &access_token).await?;
    ensure_identity_mail_provider(&client, &access_token).await?;
    ensure_branding(&client, &access_token).await?;
    let web = ensure_app(
        &client,
        &access_token,
        &project_id,
        WEB_APP,
        web_oidc_config(),
    )
    .await?;
    let android = ensure_app(
        &client,
        &access_token,
        &project_id,
        ANDROID_APP,
        native_oidc_config(),
    )
    .await?;
    let ios = ensure_app(
        &client,
        &access_token,
        &project_id,
        IOS_APP,
        native_oidc_config(),
    )
    .await?;
    ensure_member_user(&client, &access_token).await?;

    write_local_toml(&local_toml(&web, &android, &ios, &project_id))?;

    println!("Done. server/config/local.toml now has the web, Android and iOS client ids.");
    println!("Sign in at {ISSUER}/ui/console as admin@postit.com / P@$$w0rd");
    println!("App users: admin@postit.com / P@$$w0rd, member@postit.com / {MEMBER_PASSWORD}");
    let registration = if allow_register {
        "allowed"
    } else {
        "disabled"
    };
    println!("Registration: {registration}");
    let second_factor = if mfa { "required" } else { "optional" };
    println!("Second factor: {second_factor} ({environment})");
    Ok(())
}

fn trusted_client() -> Result<reqwest::Client> {
    let settings = HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(15),
        user_agent: "postit-xtask/0".to_string(),
        extra_ca_files: vec![PathBuf::from(CA_PATH)],
    };
    postit_http::build_client(&settings).context("building a client trusting the dev CA")
}

async fn wait_for_discovery(client: &reqwest::Client) -> Result<()> {
    let url = format!("{ISSUER}/.well-known/openid-configuration");
    for attempt in 1..=60 {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return Ok(());
        }
        if attempt == 60 {
            bail!(
                "Zitadel never became reachable at {url}. Is the dev stack up (`./stack.ps1 up` or `./stack.sh up` from the repo root)?"
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}

fn load_or_capture_machine_key() -> Result<MachineKey> {
    let path = Path::new(MACHINE_KEY_PATH);
    if !path.exists() {
        let raw = capture_machine_key_from_logs()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &raw).with_context(|| format!("writing {}", path.display()))?;
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn capture_machine_key_from_logs() -> Result<String> {
    // `docker logs` keeps the container's stdout and stderr apart, so search both.
    let output = std::process::Command::new("docker")
        .args(["logs", ZITADEL_CONTAINER])
        .output()
        .with_context(|| format!("running `docker logs {ZITADEL_CONTAINER}`"))?;
    let logs = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    logs.lines()
        .rev()
        .find_map(|line| {
            let start = line.find(r#"{"type":"serviceaccount""#)?;
            Some(line[start..].to_string())
        })
        .context(
            "no machine key found in `docker logs postit-zitadel`; Zitadel's FirstInstance \
             setup only prints it once, on the run that created the machine user",
        )
}

async fn machine_bearer_token(
    client: &reqwest::Client,
    machine_key: &MachineKey,
) -> Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let claims = Claims {
        iss: machine_key.user_id.clone(),
        sub: machine_key.user_id.clone(),
        aud: ISSUER.to_string(),
        iat: now,
        exp: now + 3600,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(machine_key.key_id.clone());
    let key = EncodingKey::from_rsa_pem(machine_key.key.as_bytes())
        .context("parsing the machine key's RSA private key")?;
    let assertion = encode(&header, &claims, &key).context("signing the JWT profile assertion")?;

    let resp = client
        .post(format!("{ISSUER}/oauth/v2/token"))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &assertion),
            ("scope", "openid urn:zitadel:iam:org:project:id:zitadel:aud"),
        ])
        .send()
        .await
        .context("requesting an access token for the bootstrap machine user")?;
    let body: Value = check(resp, "authenticating the bootstrap machine user").await?;
    body["access_token"]
        .as_str()
        .map(str::to_string)
        .context("token response had no access_token")
}

async fn ensure_project(client: &reqwest::Client, token: &str) -> Result<String> {
    let resp = client
        .post(format!("{ISSUER}/management/v1/projects"))
        .bearer_auth(token)
        .json(&json!({ "name": PROJECT_NAME }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let resp = client
            .post(format!("{ISSUER}/management/v1/projects/_search"))
            .bearer_auth(token)
            .json(&json!({
                "queries": [{ "nameQuery": { "name": PROJECT_NAME, "method": "TEXT_QUERY_METHOD_EQUALS" } }]
            }))
            .send()
            .await?;
        let body: Value = check(resp, "looking up the existing postit project").await?;
        return body["result"][0]["id"]
            .as_str()
            .map(str::to_string)
            .context("project search returned no result");
    }
    let body: Value = check(resp, "creating the postit project").await?;
    body["id"]
        .as_str()
        .map(str::to_string)
        .context("project creation returned no id")
}

fn web_oidc_config() -> Value {
    json!({
        "redirectUris": WEB_REDIRECT_URIS,
        "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
        "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
        "appType": "OIDC_APP_TYPE_USER_AGENT",
        "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
        "postLogoutRedirectUris": WEB_POST_LOGOUT_REDIRECT_URIS,
        "devMode": true,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
    })
}

/// A native application: Zitadel accepts custom-scheme redirects for this type without
/// development mode.
fn native_oidc_config() -> Value {
    json!({
        "redirectUris": MOBILE_REDIRECT_URIS,
        "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
        "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
        "appType": "OIDC_APP_TYPE_NATIVE",
        "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
        "postLogoutRedirectUris": MOBILE_POST_LOGOUT_REDIRECT_URIS,
        "devMode": false,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
    })
}

async fn ensure_app(
    client: &reqwest::Client,
    token: &str,
    project_id: &str,
    name: &str,
    config: Value,
) -> Result<String> {
    let mut create = config.clone();
    create["name"] = json!(name);
    let resp = client
        .post(format!(
            "{ISSUER}/management/v1/projects/{project_id}/apps/oidc"
        ))
        .bearer_auth(token)
        .json(&create)
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let resp = client
            .post(format!(
                "{ISSUER}/management/v1/projects/{project_id}/apps/_search"
            ))
            .bearer_auth(token)
            .json(&json!({
                "queries": [{ "nameQuery": { "name": name, "method": "TEXT_QUERY_METHOD_EQUALS" } }]
            }))
            .send()
            .await?;
        let body: Value =
            check(resp, &format!("looking up the existing {name} application")).await?;
        let app = &body["result"][0];
        let app_id = app["id"].as_str().context("app search returned no id")?;
        let client_id = app["oidcConfig"]["clientId"]
            .as_str()
            .context("app search returned no clientId")?;
        // Bring an app created by an older run up to date, so a re-run adds new redirect URIs.
        let resp = client
            .put(format!(
                "{ISSUER}/management/v1/projects/{project_id}/apps/{app_id}/oidc_config"
            ))
            .bearer_auth(token)
            .json(&config)
            .send()
            .await?;
        if resp.status() != reqwest::StatusCode::BAD_REQUEST {
            // Zitadel answers 400 `COMMAND-1m88i` when nothing changed.
            check::<Value>(resp, &format!("updating the {name} redirect URIs")).await?;
        }
        return Ok(client_id.to_string());
    }
    let body: Value = check(resp, &format!("creating the {name} OIDC application")).await?;
    body["clientId"]
        .as_str()
        .map(str::to_string)
        .context("app creation returned no clientId")
}

/// Zitadel v4 redirects sign-in to `/ui/v2/login`, a separate app (`zitadel-login`) this
/// stack does not run, so authorize requests would land on a 404. Not requiring login v2
/// keeps Zitadel's built-in login, which the same container serves.
async fn ensure_builtin_login(client: &reqwest::Client, token: &str) -> Result<()> {
    let resp = client
        .put(format!("{ISSUER}/v2/features/instance"))
        .bearer_auth(token)
        .json(&json!({ "loginV2": { "required": false } }))
        .send()
        .await?;
    check::<Value>(resp, "disabling the separate login v2 app").await?;
    Ok(())
}

/// `ZITADEL_ALLOW_REGISTRATION`: unset or empty allows self-registration.
fn allow_registration(value: Option<&str>) -> Result<bool> {
    bool_setting("ZITADEL_ALLOW_REGISTRATION", value, true)
}

/// `ZITADEL_FORCE_MFA`: unset or empty forces a second factor everywhere but development.
fn force_mfa(value: Option<&str>, environment: Environment) -> Result<bool> {
    bool_setting(
        "ZITADEL_FORCE_MFA",
        value,
        environment != Environment::Development,
    )
}

/// A true/false setting named `name`; unset or empty gives `default`.
fn bool_setting(name: &str, value: Option<&str>, default: bool) -> Result<bool> {
    match value
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "" => Ok(default),
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => bail!(
            "{name} must be true or false, got {:?}",
            value.unwrap_or_default()
        ),
    }
}

const LOGIN_POLICY_PATH: &str = "/management/v1/policies/login";

/// The org's login policy with registration set and a second factor either forced on every
/// sign-in (`force_mfa`) or never forced or prompted for. The
/// org starts on the instance default (`isDefault`), which is replaced by a custom policy
/// (POST) once; later runs update that custom policy (PUT). Zitadel omits false fields,
/// so a missing `isDefault` means custom.
fn login_policy_request(
    current: &Value,
    allow_register: bool,
    force_mfa: bool,
) -> (reqwest::Method, &'static str, Value) {
    let mut policy = current["policy"].clone();
    let is_default = policy["isDefault"].as_bool().unwrap_or(false);
    if let Some(fields) = policy.as_object_mut() {
        for read_only in ["isDefault", "details", "id"] {
            fields.remove(read_only);
        }
        fields.insert("allowRegister".into(), json!(allow_register));
        fields.insert("forceMfa".into(), json!(force_mfa));
        // Forced for every sign-in, not only username and password ones.
        fields.insert("forceMfaLocalOnly".into(), json!(false));
        if !force_mfa {
            // A zero skip lifetime means Zitadel never asks to set up a second factor.
            fields.insert("mfaInitSkipLifetime".into(), json!("0s"));
        }
    }
    let method = if is_default {
        reqwest::Method::POST
    } else {
        reqwest::Method::PUT
    };
    (method, LOGIN_POLICY_PATH, policy)
}

async fn ensure_login_policy(
    client: &reqwest::Client,
    token: &str,
    allow_register: bool,
    force_mfa: bool,
) -> Result<()> {
    let resp = client
        .get(format!("{ISSUER}{LOGIN_POLICY_PATH}"))
        .bearer_auth(token)
        .send()
        .await?;
    let current: Value = check(resp, "reading the login policy").await?;
    let (method, path, body) = login_policy_request(&current, allow_register, force_mfa);
    let resp = client
        .request(method, format!("{ISSUER}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        let text = resp.text().await.unwrap_or_default();
        // Zitadel answers 400 `Errors.Org.LoginPolicy.NotChanged` when nothing changed.
        if text.contains("NotChanged") {
            return Ok(());
        }
        bail!("setting the login policy failed: 400 Bad Request: {text}");
    }
    check::<Value>(resp, "setting the login policy").await?;
    Ok(())
}

/// Marks the SMTP provider this task owns, so re-runs find it instead of adding another.
const SMTP_DESCRIPTION: &str = "postit dev: host SMTP tool";

/// Zitadel's own mail (email verification, password reset) through the operator's local
/// SMTP tool (e.g. Papercut) on the Docker host, the same one the postit server uses.
fn smtp_provider_body() -> Value {
    json!({
        "senderAddress": "noreply@postit.local",
        "senderName": "postit",
        "tls": false,
        "host": "host.docker.internal:25",
        "description": SMTP_DESCRIPTION,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum SmtpStep {
    Create,
    /// Ours exists; it stays the inactive fallback, so it is never activated here.
    Keep,
}

/// What to do given an `_search` result. Ours is found by its description.
fn smtp_step(list: &Value) -> SmtpStep {
    if providers_of(list)
        .iter()
        .any(|p| p["description"] == SMTP_DESCRIPTION)
    {
        SmtpStep::Keep
    } else {
        SmtpStep::Create
    }
}

fn providers_of(list: &Value) -> &[Value] {
    list["result"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

async fn list_email_providers(client: &reqwest::Client, token: &str) -> Result<Value> {
    let resp = client
        .post(format!("{ISSUER}/admin/v1/email/_search"))
        .bearer_auth(token)
        .json(&json!({}))
        .send()
        .await?;
    check(resp, "listing the email providers").await
}

/// Adds the SMTP provider when missing. It is not activated: postit's webhook provider is
/// the active one, and this stays as the inactive fallback.
async fn ensure_smtp_provider(client: &reqwest::Client, token: &str) -> Result<()> {
    let list = list_email_providers(client, token).await?;
    if smtp_step(&list) == SmtpStep::Keep {
        return Ok(());
    }
    println!("POST /admin/v1/email/smtp (create the inactive SMTP fallback)");
    let resp = client
        .post(format!("{ISSUER}/admin/v1/email/smtp"))
        .bearer_auth(token)
        .json(&smtp_provider_body())
        .send()
        .await?;
    check::<Value>(resp, "adding the SMTP provider").await?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderStep {
    /// `POST /admin/v1/email/http`, which returns the signing key.
    Create,
    /// `PUT /admin/v1/email/http/{id}`, no rotation.
    UpdateEndpoint(String),
    /// The same `PUT` with `"expirationSigningKey": "0s"`, which returns a new signing key.
    RotateKey(String),
    /// `POST /admin/v1/email/{id}/_activate`.
    Activate(String),
    /// An active provider that is neither of ours: change nothing and warn.
    KeepOperatorProvider,
}

/// The steps to make our HTTP provider the active one, in order, given the `_search`
/// result, whether the vault already holds the signing key, and the webhook endpoint.
/// The caller always activates the id that `Create` returns, so `Create` is never
/// followed by `Activate` here.
fn provider_plan(list: &Value, vault_has_key: bool, endpoint: &str) -> Vec<ProviderStep> {
    let providers = providers_of(list);
    let is_active = |p: &Value| p["state"] == "EMAIL_PROVIDER_ACTIVE";
    if providers.iter().any(|p| {
        is_active(p)
            && p["description"] != IDENTITY_MAIL_DESCRIPTION
            && p["description"] != SMTP_DESCRIPTION
    }) {
        return vec![ProviderStep::KeepOperatorProvider];
    }
    let Some(ours) = providers
        .iter()
        .find(|p| p["description"] == IDENTITY_MAIL_DESCRIPTION)
    else {
        return vec![ProviderStep::Create];
    };
    let Some(id) = ours["id"].as_str().map(str::to_string) else {
        return vec![ProviderStep::Create];
    };
    let mut steps = Vec::new();
    if !vault_has_key {
        // A rotation sends the endpoint too, so it covers a changed endpoint.
        steps.push(ProviderStep::RotateKey(id.clone()));
    } else if ours["http"]["endpoint"] != endpoint {
        steps.push(ProviderStep::UpdateEndpoint(id.clone()));
    }
    if !is_active(ours) {
        steps.push(ProviderStep::Activate(id));
    }
    steps
}

/// The webhook endpoint Zitadel posts identity mail to.
fn identity_mail_endpoint() -> String {
    std::env::var("ZITADEL_IDENTITY_MAIL_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_IDENTITY_MAIL_URL.to_string())
}

/// The contents of a dotenv file, or an error naming the path when it is missing.
fn read_env_file(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "{} does not exist; extract !ref/vault.7z (`7z x vault.7z` from !ref/) first",
            path.display()
        ),
        other => other.with_context(|| format!("reading {}", path.display())),
    }
}

/// The value of `key` on a `KEY=value` line, without surrounding quotes.
fn env_line_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let value = line.strip_prefix(key)?.strip_prefix('=')?.trim();
    Some(value.trim_matches(|c| c == '\'' || c == '"'))
}

/// Whether the dotenv file sets `key` to a non-empty value.
fn env_has_value(path: &Path, key: &str) -> Result<bool> {
    let content = read_env_file(path)?;
    Ok(content
        .lines()
        .any(|line| env_line_value(line, key).is_some_and(|v| !v.is_empty())))
}

/// Replaces or appends `KEY='value'` in a dotenv file. Other lines and their order stay.
fn upsert_env_value(path: &Path, key: &str, value: &str) -> Result<()> {
    if value.contains(['\'', '\n', '\r']) {
        bail!("the value for {key} cannot be single-quoted");
    }
    let content = read_env_file(path)?;
    let eol = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let new_line = format!("{key}='{value}'");
    let mut replaced = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if !replaced && env_line_value(line, key).is_some() {
                replaced = true;
                new_line.clone()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !replaced {
        lines.push(new_line);
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))
}

async fn put_http_provider(
    client: &reqwest::Client,
    token: &str,
    id: &str,
    endpoint: &str,
    rotate: bool,
) -> Result<Value> {
    let mut body = json!({ "endpoint": endpoint, "description": IDENTITY_MAIL_DESCRIPTION });
    if rotate {
        body["expirationSigningKey"] = json!("0s");
    }
    let resp = client
        .put(format!("{ISSUER}/admin/v1/email/http/{id}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    check(resp, "updating the webhook email provider").await
}

async fn activate_provider(client: &reqwest::Client, token: &str, id: &str) -> Result<()> {
    println!("POST /admin/v1/email/{id}/_activate");
    let resp = client
        .post(format!("{ISSUER}/admin/v1/email/{id}/_activate"))
        .bearer_auth(token)
        .json(&json!({}))
        .send()
        .await?;
    check::<Value>(resp, "activating the webhook email provider").await?;
    Ok(())
}

/// Stores a signing key Zitadel just returned. The key is never printed.
fn store_signing_key(response: &Value) -> Result<()> {
    let key = response["signingKey"]
        .as_str()
        .filter(|k| !k.is_empty())
        .context("Zitadel returned no signingKey")?;
    upsert_env_value(Path::new(VAULT_POSTIT_ENV), SIGNING_KEY_VAR, key)?;
    println!(
        "Wrote {SIGNING_KEY_VAR} to !ref/vault/development/postit.env; restart postit and re-pack vault.7z"
    );
    Ok(())
}

/// Makes the HTTP provider that delivers Zitadel's mail to postit the active one.
async fn ensure_identity_mail_provider(client: &reqwest::Client, token: &str) -> Result<()> {
    let endpoint = identity_mail_endpoint();
    let has_key = env_has_value(Path::new(VAULT_POSTIT_ENV), SIGNING_KEY_VAR)?;
    let list = list_email_providers(client, token).await?;
    for step in provider_plan(&list, has_key, &endpoint) {
        match step {
            ProviderStep::KeepOperatorProvider => {
                eprintln!(
                    "warning: an email provider you set up is active; leaving it. postit's \
                     webhook provider is not used until you activate it in the Zitadel console."
                );
            }
            ProviderStep::Create => {
                println!("POST /admin/v1/email/http (create the webhook provider)");
                let resp = client
                    .post(format!("{ISSUER}/admin/v1/email/http"))
                    .bearer_auth(token)
                    .json(&json!({
                        "endpoint": endpoint,
                        "description": IDENTITY_MAIL_DESCRIPTION,
                    }))
                    .send()
                    .await?;
                let created: Value = check(resp, "adding the webhook email provider").await?;
                store_signing_key(&created)?;
                let id = created["id"]
                    .as_str()
                    .context("webhook provider creation returned no id")?;
                activate_provider(client, token, id).await?;
            }
            ProviderStep::UpdateEndpoint(id) => {
                println!("PUT /admin/v1/email/http/{id} (update the endpoint)");
                put_http_provider(client, token, &id, &endpoint, false).await?;
            }
            ProviderStep::RotateKey(id) => {
                println!("PUT /admin/v1/email/http/{id} (rotate the signing key)");
                let rotated = put_http_provider(client, token, &id, &endpoint, true).await?;
                store_signing_key(&rotated)?;
            }
            ProviderStep::Activate(id) => activate_provider(client, token, &id).await?,
        }
    }
    Ok(())
}

/// Installs postit's texts for each identity message, in English and Arabic. Zitadel
/// answers 200 to a text that changes nothing, so a re-run needs no special handling.
async fn ensure_message_texts(client: &reqwest::Client, token: &str) -> Result<()> {
    for language in MESSAGE_LANGUAGES {
        let path = Path::new(MESSAGES_DIR).join(format!("{language}.yaml"));
        let texts = zitadel_messages::load(&path)?;
        let requests = zitadel_messages::requests(language, &texts);
        for (route, body) in &requests {
            let resp = client
                .put(format!("{ISSUER}{route}"))
                .bearer_auth(token)
                .json(body)
                .send()
                .await?;
            check::<Value>(resp, &format!("setting {route}")).await?;
        }
        println!(
            "PUT /admin/v1/text/message/*/{language} ({} message texts)",
            requests.len()
        );
    }
    Ok(())
}

/// `--check`: lists the providers and reports the texts, and fails when postit's webhook
/// provider is not the active one.
async fn check_identity_mail(client: &reqwest::Client, token: &str) -> Result<()> {
    let list = list_email_providers(client, token).await?;
    for p in providers_of(&list) {
        println!(
            "provider {} ({}): {}",
            p["id"].as_str().unwrap_or("?"),
            p["description"].as_str().unwrap_or("?"),
            p["state"].as_str().unwrap_or("?")
        );
    }
    for language in MESSAGE_LANGUAGES {
        let path = Path::new(MESSAGES_DIR).join(format!("{language}.yaml"));
        let texts = zitadel_messages::load(&path)?;
        let mut stale = Vec::new();
        for (kind, text) in &texts {
            let resp = client
                .get(format!("{ISSUER}/admin/v1/text/message/{kind}/{language}"))
                .bearer_auth(token)
                .send()
                .await?;
            let current: Value = check(resp, "reading a message text").await?;
            if current["customText"]["subject"] != text.subject.as_str() {
                stale.push(kind.as_str());
            }
        }
        if stale.is_empty() {
            println!("texts {language}: {} up to date", texts.len());
        } else {
            println!("texts {language}: out of date: {}", stale.join(", "));
        }
    }
    let active_is_ours = providers_of(&list).iter().any(|p| {
        p["state"] == "EMAIL_PROVIDER_ACTIVE" && p["description"] == IDENTITY_MAIL_DESCRIPTION
    });
    if !active_is_ours {
        bail!(
            "postit's webhook email provider is not the active one; run `cargo xtask zitadel-bootstrap`"
        );
    }
    Ok(())
}

const LABEL_POLICY_PATH: &str = "/admin/v1/policies/label";

/// postit's brand colors, light and dark.
fn label_policy_body() -> Value {
    json!({
        "primaryColor": "#df528a",
        "backgroundColor": "#f8fafc",
        "fontColor": "#1f1e24",
        "warnColor": "#b91c1c",
        "primaryColorDark": "#f27aa6",
        "backgroundColorDark": "#1f1e24",
        "fontColorDark": "#f8fafc",
        "warnColorDark": "#f87171",
        "hideLoginNameSuffix": false,
        "disableWatermark": false,
        "themeMode": "THEME_MODE_AUTO",
    })
}

/// A label policy without the fields that change on every read.
fn policy_of(response: &Value) -> Value {
    let mut policy = response["policy"].clone();
    if let Some(fields) = policy.as_object_mut() {
        fields.remove("details");
    }
    policy
}

async fn get_label_policy(client: &reqwest::Client, token: &str, preview: bool) -> Result<Value> {
    let suffix = if preview { "/_preview" } else { "" };
    let resp = client
        .get(format!("{ISSUER}{LABEL_POLICY_PATH}{suffix}"))
        .bearer_auth(token)
        .send()
        .await?;
    check(resp, "reading the label policy").await
}

async fn upload_asset(
    client: &reqwest::Client,
    token: &str,
    kind: &str,
    path: &Path,
    mime: &str,
) -> Result<()> {
    println!("POST /assets/v1/instance/policy/label/{kind}");
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_name()
        .map_or_else(|| kind.to_string(), |n| n.to_string_lossy().into_owned());
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(name)
        .mime_str(mime)?;
    let resp = client
        .post(format!("{ISSUER}/assets/v1/instance/policy/label/{kind}"))
        .bearer_auth(token)
        .multipart(reqwest::multipart::Form::new().part("file", part))
        .send()
        .await?;
    // A successful upload answers with an empty body.
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("uploading the {kind} failed: {status}: {text}");
    }
    Ok(())
}

/// Applies postit's colors, logo, and font to the instance label policy and activates it
/// when the pending policy differs from the active one. Zitadel answers 400
/// `INSTANCE-28fHe` to a policy PUT that changes nothing, which counts as success; the
/// logo and font are uploaded only while the pending policy has none.
async fn ensure_branding(client: &reqwest::Client, token: &str) -> Result<()> {
    let resp = client
        .put(format!("{ISSUER}{LABEL_POLICY_PATH}"))
        .bearer_auth(token)
        .json(&label_policy_body())
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        let text = resp.text().await.unwrap_or_default();
        if !text.contains("INSTANCE-28fHe") {
            bail!("setting the label policy failed: 400 Bad Request: {text}");
        }
    } else {
        println!("PUT {LABEL_POLICY_PATH} (set the brand colors)");
        check::<Value>(resp, "setting the label policy").await?;
    }
    let pending = policy_of(&get_label_policy(client, token, true).await?);
    let has = |field: &str| pending[field].as_str().is_some_and(|v| !v.is_empty());
    if !has("logoUrl") {
        let logo = Path::new(BRANDING_LOGO_PATH);
        upload_asset(client, token, "logo", logo, "image/svg+xml").await?;
    }
    if !has("fontUrl") {
        let font = Path::new(BRANDING_FONT_PATH);
        upload_asset(client, token, "font", font, "font/ttf").await?;
    }
    let pending = policy_of(&get_label_policy(client, token, true).await?);
    let active = policy_of(&get_label_policy(client, token, false).await?);
    if pending != active {
        println!("POST {LABEL_POLICY_PATH}/_activate");
        let resp = client
            .post(format!("{ISSUER}{LABEL_POLICY_PATH}/_activate"))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await?;
        check::<Value>(resp, "activating the label policy").await?;
    }
    Ok(())
}

/// Loads `docker/.env` into the process environment without overriding variables already
/// set. A missing file is fine; a malformed one is an error, because dotenvy stops at the
/// first bad line and every setting below it would be silently ignored.
fn load_env_file(path: &Path) -> Result<()> {
    match dotenvy::from_path(path) {
        Err(e) if !e.not_found() => Err(e).with_context(|| format!("reading {}", path.display())),
        _ => Ok(()),
    }
}

async fn ensure_member_user(client: &reqwest::Client, token: &str) -> Result<()> {
    let resp = client
        .post(format!("{ISSUER}/management/v1/users/human/_import"))
        .bearer_auth(token)
        .json(&json!({
            "userName": MEMBER_EMAIL,
            "profile": { "firstName": "Postit", "lastName": "Member" },
            "email": { "email": MEMBER_EMAIL, "isEmailVerified": true },
            "password": MEMBER_PASSWORD,
            "passwordChangeRequired": false,
        }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        return Ok(());
    }
    check::<Value>(resp, "creating the member@postit.com user").await?;
    Ok(())
}

async fn check<T: serde::de::DeserializeOwned>(resp: reqwest::Response, action: &str) -> Result<T> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("{action} failed: {status}: {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("{action}: parsing response: {text}"))
}

fn local_toml(web: &str, android: &str, ios: &str, project_id: &str) -> String {
    format!(
        r#"# Generated by `cargo xtask zitadel-bootstrap`. Safe to delete and regenerate;
# re-running the bootstrap task is idempotent. Git-ignored (see .gitignore).

[auth.oidc]
client_id = "{web}"
android_client_id = "{android}"
ios_client_id = "{ios}"
audiences = ["{web}", "{project_id}"]
"#
    )
}

fn write_local_toml(contents: &str) -> Result<()> {
    let path = Path::new(LOCAL_TOML_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_toml_lists_every_client() {
        let toml = local_toml("web-1", "android-1", "ios-1", "project-1");
        assert!(toml.contains("client_id = \"web-1\""), "{toml}");
        assert!(toml.contains("android_client_id = \"android-1\""), "{toml}");
        assert!(toml.contains("ios_client_id = \"ios-1\""), "{toml}");
        assert!(
            toml.contains("audiences = [\"web-1\", \"project-1\"]"),
            "{toml}"
        );
    }

    #[test]
    fn native_apps_use_custom_schemes_without_dev_mode() {
        let config = native_oidc_config();
        assert_eq!(config["appType"], "OIDC_APP_TYPE_NATIVE");
        assert_eq!(config["authMethodType"], "OIDC_AUTH_METHOD_TYPE_NONE");
        assert_eq!(config["devMode"], false);
        assert_eq!(config["redirectUris"][0], "postit-dev://auth/callback");
        assert_eq!(
            config["postLogoutRedirectUris"][0],
            "postit-dev://auth/logout"
        );
        assert_eq!(config["accessTokenType"], "OIDC_TOKEN_TYPE_JWT");
    }

    #[test]
    fn web_app_keeps_only_web_redirects() {
        let config = web_oidc_config();
        let uris = config["redirectUris"].as_array().map(Vec::len);
        assert_eq!(uris, Some(2));
        assert_eq!(config["appType"], "OIDC_APP_TYPE_USER_AGENT");
    }

    #[test]
    fn allow_registration_defaults_to_true() {
        assert!(allow_registration(None).unwrap_or_else(|e| unreachable!("{e}")));
        assert!(allow_registration(Some("")).unwrap_or_else(|e| unreachable!("{e}")));
    }

    #[test]
    fn allow_registration_reads_true_and_false() {
        for (v, want) in [
            ("true", true),
            ("TRUE", true),
            ("1", true),
            ("false", false),
            ("False", false),
            ("0", false),
        ] {
            assert_eq!(
                allow_registration(Some(v)).unwrap_or_else(|e| unreachable!("{e}")),
                want,
                "{v}"
            );
        }
    }

    #[test]
    fn allow_registration_rejects_other_values() {
        let err = allow_registration(Some("maybe"))
            .err()
            .unwrap_or_else(|| unreachable!("accepted"));
        assert!(
            err.to_string().contains("ZITADEL_ALLOW_REGISTRATION"),
            "{err}"
        );
    }

    #[test]
    fn force_mfa_defaults_to_off_in_development_only() {
        for (env, want) in [
            (Environment::Development, false),
            (Environment::Qa, true),
            (Environment::Production, true),
        ] {
            assert_eq!(
                force_mfa(None, env).unwrap_or_else(|e| unreachable!("{e}")),
                want,
                "{env}"
            );
            assert_eq!(
                force_mfa(Some(" "), env).unwrap_or_else(|e| unreachable!("{e}")),
                want,
                "{env}"
            );
        }
    }

    #[test]
    fn force_mfa_setting_overrides_the_environment_default() {
        assert!(
            force_mfa(Some("true"), Environment::Development)
                .unwrap_or_else(|e| unreachable!("{e}"))
        );
        assert!(
            !force_mfa(Some("0"), Environment::Production).unwrap_or_else(|e| unreachable!("{e}"))
        );
    }

    #[test]
    fn force_mfa_rejects_other_values() {
        let err = force_mfa(Some("yes please"), Environment::Qa)
            .err()
            .unwrap_or_else(|| unreachable!("accepted"));
        assert!(err.to_string().contains("ZITADEL_FORCE_MFA"), "{err}");
    }

    #[test]
    fn login_policy_request_forces_mfa_when_asked() {
        let current = json!({ "policy": { "isDefault": true, "forceMfa": false,
            "mfaInitSkipLifetime": "0s", "secondFactors": ["SECOND_FACTOR_TYPE_OTP"] } });
        let (_, _, body) = login_policy_request(&current, true, true);
        assert_eq!(body["forceMfa"], json!(true));
        // Forced means every sign-in without a second factor must set one up first.
        assert_eq!(body["forceMfaLocalOnly"], json!(false));
        assert_eq!(body["secondFactors"], json!(["SECOND_FACTOR_TYPE_OTP"]));
    }

    #[test]
    fn login_policy_request_adds_a_custom_policy_over_the_default() {
        let current = json!({ "policy": { "isDefault": true, "allowUsernamePassword": true, "allowRegister": true,
            "forceMfa": true, "mfaInitSkipLifetime": "2592000s", "passwordlessType": "PASSWORDLESS_TYPE_ALLOWED" } });
        let (method, path, body) = login_policy_request(&current, false, false);
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(path, "/management/v1/policies/login");
        assert_eq!(body["allowRegister"], json!(false));
        assert_eq!(body["forceMfa"], json!(false));
        assert_eq!(body["mfaInitSkipLifetime"], json!("0s"));
        assert_eq!(body["allowUsernamePassword"], json!(true));
        assert_eq!(body["passwordlessType"], json!("PASSWORDLESS_TYPE_ALLOWED"));
        assert!(body.get("isDefault").is_none());
    }

    #[test]
    fn login_policy_request_updates_a_custom_policy() {
        let current = json!({ "policy": { "isDefault": false, "allowRegister": false, "forceMfa": false,
            "mfaInitSkipLifetime": "0s" } });
        let (method, path, body) = login_policy_request(&current, true, false);
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(path, "/management/v1/policies/login");
        assert_eq!(body["allowRegister"], json!(true));
    }

    #[test]
    fn login_policy_request_reads_a_missing_is_default_as_custom() {
        // Zitadel's JSON leaves out false values, so a custom policy has no `isDefault`.
        let current = json!({ "policy": { "allowRegister": true } });
        let (method, _, _) = login_policy_request(&current, true, false);
        assert_eq!(method, reqwest::Method::PUT);
    }

    #[test]
    fn smtp_provider_points_at_the_host_smtp_tool() {
        let body = smtp_provider_body();
        assert_eq!(body["host"], json!("host.docker.internal:25"));
        assert_eq!(body["tls"], json!(false));
        assert_eq!(body["senderAddress"], json!("noreply@postit.local"));
        assert_eq!(body["description"], json!(SMTP_DESCRIPTION));
    }

    #[test]
    fn smtp_step_creates_ours_only_when_missing() {
        assert_eq!(smtp_step(&json!({})), SmtpStep::Create);
        let inactive_other = json!({ "result": [{ "id": "x", "description": "theirs" }] });
        assert_eq!(smtp_step(&inactive_other), SmtpStep::Create);
    }

    #[test]
    fn smtp_step_never_activates_ours() {
        // Ours stays the inactive fallback, whatever state it is in.
        for state in ["EMAIL_PROVIDER_INACTIVE", "EMAIL_PROVIDER_ACTIVE"] {
            let list = json!({ "result": [
                { "id": "ours", "description": SMTP_DESCRIPTION, "state": state },
            ] });
            assert_eq!(smtp_step(&list), SmtpStep::Keep, "{state}");
        }
    }

    const ENDPOINT: &str = "http://host.docker.internal:44312/internal/identity-mail";

    fn ours(state: &str, endpoint: &str) -> Value {
        json!({ "id": "ours", "description": IDENTITY_MAIL_DESCRIPTION, "state": state,
            "http": { "endpoint": endpoint } })
    }

    fn smtp(state: &str) -> Value {
        json!({ "id": "smtp", "description": SMTP_DESCRIPTION, "state": state, "smtp": {} })
    }

    fn list(providers: impl IntoIterator<Item = Value>) -> Value {
        json!({ "result": providers.into_iter().collect::<Vec<_>>() })
    }

    #[test]
    fn provider_plan_creates_when_missing() {
        assert_eq!(
            provider_plan(&list(vec![]), true, ENDPOINT),
            [ProviderStep::Create]
        );
        // Our SMTP provider active: the caller activates what Create returns.
        assert_eq!(
            provider_plan(&list(vec![smtp("EMAIL_PROVIDER_ACTIVE")]), false, ENDPOINT),
            [ProviderStep::Create]
        );
    }

    #[test]
    fn provider_plan_activates_an_inactive_provider() {
        let l = list(vec![
            smtp("EMAIL_PROVIDER_ACTIVE"),
            ours("EMAIL_PROVIDER_INACTIVE", ENDPOINT),
        ]);
        assert_eq!(
            provider_plan(&l, true, ENDPOINT),
            [ProviderStep::Activate("ours".into())]
        );
    }

    #[test]
    fn provider_plan_is_empty_when_all_is_in_place() {
        let l = list(vec![ours("EMAIL_PROVIDER_ACTIVE", ENDPOINT)]);
        assert_eq!(provider_plan(&l, true, ENDPOINT), []);
    }

    #[test]
    fn provider_plan_updates_a_changed_endpoint() {
        let l = list(vec![ours("EMAIL_PROVIDER_ACTIVE", "http://old/x")]);
        assert_eq!(
            provider_plan(&l, true, ENDPOINT),
            [ProviderStep::UpdateEndpoint("ours".into())]
        );
    }

    #[test]
    fn provider_plan_rotates_when_the_vault_lost_the_key() {
        let l = list(vec![ours("EMAIL_PROVIDER_ACTIVE", ENDPOINT)]);
        assert_eq!(
            provider_plan(&l, false, ENDPOINT),
            [ProviderStep::RotateKey("ours".into())]
        );
        // A rotation also carries the endpoint, so no separate update.
        let changed = list(vec![ours("EMAIL_PROVIDER_ACTIVE", "http://old/x")]);
        assert_eq!(
            provider_plan(&changed, false, ENDPOINT),
            [ProviderStep::RotateKey("ours".into())]
        );
    }

    #[test]
    fn provider_plan_fixes_then_activates_an_inactive_provider() {
        let l = list(vec![
            smtp("EMAIL_PROVIDER_ACTIVE"),
            ours("EMAIL_PROVIDER_INACTIVE", "http://old/x"),
        ]);
        assert_eq!(
            provider_plan(&l, false, ENDPOINT),
            [
                ProviderStep::RotateKey("ours".into()),
                ProviderStep::Activate("ours".into())
            ]
        );
    }

    #[test]
    fn provider_plan_leaves_an_operator_provider_alone() {
        let theirs =
            json!({ "id": "theirs", "description": "console", "state": "EMAIL_PROVIDER_ACTIVE" });
        let l = list(vec![theirs, ours("EMAIL_PROVIDER_INACTIVE", ENDPOINT)]);
        assert_eq!(
            provider_plan(&l, true, ENDPOINT),
            [ProviderStep::KeepOperatorProvider]
        );
    }

    fn env_file(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.path().join("postit.env");
        std::fs::write(&path, content).unwrap_or_else(|e| unreachable!("{e}"));
        (dir, path)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{e}"))
    }

    #[test]
    fn upsert_env_value_replaces_in_place_and_keeps_the_rest() {
        let (_d, path) = env_file(
            "# head
A=1
KEY='old'

# tail
B=2
",
        );
        upsert_env_value(&path, "KEY", "new").unwrap_or_else(|e| unreachable!("{e:#}"));
        assert_eq!(
            read(&path),
            "# head
A=1
KEY='new'

# tail
B=2
"
        );
    }

    #[test]
    fn upsert_env_value_appends_after_a_missing_trailing_newline() {
        let (_d, path) = env_file("A=1");
        upsert_env_value(&path, "KEY", "v").unwrap_or_else(|e| unreachable!("{e:#}"));
        assert_eq!(
            read(&path),
            "A=1
KEY='v'
"
        );
    }

    #[test]
    fn upsert_env_value_does_not_touch_similar_keys_or_comments() {
        let (_d, path) = env_file(
            "# KEY=commented
KEY_2=x
",
        );
        upsert_env_value(&path, "KEY", "v").unwrap_or_else(|e| unreachable!("{e:#}"));
        assert_eq!(
            read(&path),
            "# KEY=commented
KEY_2=x
KEY='v'
"
        );
    }

    #[test]
    fn upsert_env_value_refuses_a_missing_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.path().join("nope.env");
        let err = upsert_env_value(&path, "KEY", "v")
            .err()
            .unwrap_or_else(|| unreachable!("accepted"));
        let text = format!("{err:#}");
        assert!(text.contains("nope.env"), "{text}");
        assert!(text.contains("extract !ref/vault.7z"), "{text}");
        assert!(!path.exists());
    }

    #[test]
    fn upsert_env_value_rejects_a_value_that_cannot_be_single_quoted() {
        let (_d, path) = env_file(
            "A=1
",
        );
        assert!(upsert_env_value(&path, "KEY", "it's").is_err());
    }

    #[test]
    fn env_has_value_sees_only_a_non_empty_value() {
        let (_d, path) = env_file(
            "A=1
B=''
C='x'
# D=y
",
        );
        for (key, want) in [
            ("A", true),
            ("B", false),
            ("C", true),
            ("D", false),
            ("E", false),
        ] {
            assert_eq!(
                env_has_value(&path, key).unwrap_or_else(|e| unreachable!("{e:#}")),
                want,
                "{key}"
            );
        }
    }

    #[test]
    fn label_policy_body_uses_brand_colors() {
        let body = label_policy_body();
        assert_eq!(body["primaryColor"], "#df528a");
        assert_eq!(body["backgroundColor"], "#f8fafc");
        assert_eq!(body["fontColor"], "#1f1e24");
        assert_eq!(body["warnColor"], "#b91c1c");
        for field in [
            "primaryColorDark",
            "backgroundColorDark",
            "fontColorDark",
            "warnColorDark",
        ] {
            assert!(
                body[field].as_str().is_some_and(|c| c.starts_with('#')),
                "{field}"
            );
        }
    }

    #[test]
    fn load_env_file_ignores_a_missing_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        assert!(load_env_file(&dir.path().join(".env")).is_ok());
    }

    #[test]
    fn load_env_file_rejects_a_malformed_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "this line has no equals sign
",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        let err = load_env_file(&path)
            .err()
            .unwrap_or_else(|| unreachable!("accepted"));
        assert!(err.to_string().contains(".env"), "{err}");
    }
}
