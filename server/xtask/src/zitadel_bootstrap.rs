//! `cargo xtask zitadel-bootstrap`, run from `server/`: waits for the dev Zitadel
//! container, creates the `postit` project, the `postit-app` web application, the
//! `postit-android` and `postit-ios` native applications, and the `member@postit.com`
//! user, then writes the generated client ids and audiences into `config/local.toml`.
//! Mobile tokens carry the project ID in `aud`, so the audiences need no mobile entry. Every step is idempotent, so re-running after `stack down`
//! and back `up` is safe. Machine-user auth uses the JWT profile (RFC 7523) against the
//! machine key Zitadel prints once during `FirstInstance` setup (see
//! `docker/zitadel/steps.yaml`), which this task captures from `docker logs postit-zitadel`
//! on its first run and caches at `../docker/zitadel/machinekey/postit-bootstrap.json`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use postit_config::HttpSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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

pub async fn run() -> Result<()> {
    let client = trusted_client()?;

    println!("Waiting for Zitadel at {ISSUER} ...");
    wait_for_discovery(&client).await?;

    let machine_key = load_or_capture_machine_key()?;
    let access_token = machine_bearer_token(&client, &machine_key).await?;

    let project_id = ensure_project(&client, &access_token).await?;
    ensure_builtin_login(&client, &access_token).await?;
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
}
